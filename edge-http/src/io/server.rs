use core::fmt::{self, Debug};
use core::mem::{self, MaybeUninit};
use core::pin::pin;

use embassy_futures::select::Either;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};

use embedded_io_async::{ErrorType, Read, Write};

use log::{debug, info, warn};

use super::{
    send_headers, send_headers_end, send_status, Body, BodyType, Error, RequestHeaders, SendBody,
};

use crate::ws::{upgrade_response_headers, MAX_BASE64_KEY_RESPONSE_LEN};
use crate::DEFAULT_MAX_HEADERS_COUNT;

#[allow(unused_imports)]
#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

pub const DEFAULT_HANDLER_TASKS_COUNT: usize = 4;
pub const DEFAULT_BUF_SIZE: usize = 2048;
pub const DEFAULT_TIMEOUT_MS: u32 = 5000;

const COMPLETION_BUF_SIZE: usize = 64;

#[allow(private_interfaces)]
pub enum Connection<'b, T, const N: usize = DEFAULT_MAX_HEADERS_COUNT> {
    Transition(TransitionState),
    Unbound(T),
    Request(RequestState<'b, T, N>),
    Response(ResponseState<T>),
}

impl<'b, T, const N: usize> Connection<'b, T, N>
where
    T: Read + Write,
{
    pub async fn new(
        buf: &'b mut [u8],
        mut io: T,
        timeout_ms: Option<u32>,
    ) -> Result<Connection<'b, T, N>, Error<T::Error>> {
        let mut request = RequestHeaders::new();

        let (buf, read_len) = {
            let timeout_ms = timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);

            let receive = pin!(request.receive(buf, &mut io, true));
            let timer = Timer::after(Duration::from_millis(timeout_ms as _));

            let result = embassy_futures::select::select(receive, timer).await;

            match result {
                Either::First(result) => result,
                Either::Second(_) => Err(Error::Timeout),
            }?
        };

        let io = Body::new(
            BodyType::from_headers(request.headers.iter()),
            buf,
            read_len,
            io,
        );

        Ok(Self::Request(RequestState { request, io }))
    }

    pub fn is_request_initiated(&self) -> bool {
        matches!(self, Self::Request(_))
    }

    pub fn split(&mut self) -> (&RequestHeaders<'b, N>, &mut Body<'b, T>) {
        let req = self.request_mut().expect("Not in request mode");

        (&req.request, &mut req.io)
    }

    pub fn headers(&self) -> Result<&RequestHeaders<'b, N>, Error<T::Error>> {
        Ok(&self.request_ref()?.request)
    }

    pub fn is_ws_upgrade_request(&self) -> Result<bool, Error<T::Error>> {
        Ok(self.headers()?.is_ws_upgrade_request())
    }

    pub async fn initiate_response(
        &mut self,
        status: u16,
        message: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<(), Error<T::Error>> {
        self.complete_request(Some(status), message, headers).await
    }

    pub async fn initiate_ws_upgrade_response(
        &mut self,
        buf: &mut [u8; MAX_BASE64_KEY_RESPONSE_LEN],
    ) -> Result<(), Error<T::Error>> {
        let headers = upgrade_response_headers(self.headers()?.headers.iter(), None, buf)?;

        self.initiate_response(101, None, &headers).await
    }

    pub fn is_response_initiated(&self) -> bool {
        matches!(self, Self::Response(_))
    }

    pub async fn complete(&mut self) -> Result<(), Error<T::Error>> {
        if self.is_request_initiated() {
            self.complete_request(Some(200), Some("OK"), &[]).await?;
        }

        if self.is_response_initiated() {
            self.complete_response().await?;
        }

        Ok(())
    }

    pub async fn complete_err(&mut self, err: &str) -> Result<(), Error<T::Error>> {
        let result = self.request_mut();

        match result {
            Ok(_) => {
                let headers = [("Connection", "Close"), ("Content-Type", "text/plain")];

                self.complete_request(Some(500), Some("Internal Error"), &headers)
                    .await?;

                let response = self.response_mut()?;

                response.write_all(err.as_bytes()).await?;
                response.finish().await?;

                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn needs_close(&self) -> bool {
        match self {
            Self::Response(response) => response.needs_close(),
            _ => true,
        }
    }

    pub fn unbind(&mut self) -> Result<&mut T, Error<T::Error>> {
        let io = self.unbind_mut();
        *self = Self::Unbound(io);

        Ok(self.io_mut())
    }

    async fn complete_request(
        &mut self,
        status: Option<u16>,
        reason: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<(), Error<T::Error>> {
        let request = self.request_mut()?;

        let mut buf = [0; COMPLETION_BUF_SIZE];
        while request.io.read(&mut buf).await? > 0 {}

        let http11 = request.request.http11.unwrap_or(false);

        let mut io = self.unbind_mut();

        let result = async {
            send_status(http11, status, reason, &mut io).await?;
            let mut body_type = send_headers(
                headers.iter().filter(|(k, v)| {
                    http11
                        || !k.eq_ignore_ascii_case("Transfer-Encoding")
                        || !v.eq_ignore_ascii_case("Chunked")
                }),
                &mut io,
            )
            .await?;

            if matches!(body_type, BodyType::Unknown) {
                if http11 {
                    send_headers(&[("Transfer-Encoding", "Chunked")], &mut io).await?;
                    body_type = BodyType::Chunked;
                } else {
                    body_type = BodyType::Close;
                }
            };

            send_headers_end(&mut io).await?;

            Ok(body_type)
        }
        .await;

        match result {
            Ok(body_type) => {
                *self = Self::Response(SendBody::new(
                    if http11 { body_type } else { BodyType::Close },
                    io,
                ));

                Ok(())
            }
            Err(e) => {
                *self = Self::Unbound(io);

                Err(e)
            }
        }
    }

    async fn complete_response(&mut self) -> Result<(), Error<T::Error>> {
        self.response_mut()?.finish().await?;

        Ok(())
    }

    fn unbind_mut(&mut self) -> T {
        let state = mem::replace(self, Self::Transition(TransitionState(())));

        match state {
            Self::Request(request) => request.io.release(),
            Self::Response(response) => response.release(),
            Self::Unbound(io) => io,
            _ => unreachable!(),
        }
    }

    fn request_mut(&mut self) -> Result<&mut RequestState<'b, T, N>, Error<T::Error>> {
        if let Self::Request(request) = self {
            Ok(request)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn request_ref(&self) -> Result<&RequestState<'b, T, N>, Error<T::Error>> {
        if let Self::Request(request) = self {
            Ok(request)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn response_mut(&mut self) -> Result<&mut SendBody<T>, Error<T::Error>> {
        if let Self::Response(response) = self {
            Ok(response)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn io_mut(&mut self) -> &mut T {
        match self {
            Self::Request(request) => request.io.as_raw_reader(),
            Self::Response(response) => response.as_raw_writer(),
            Self::Unbound(io) => io,
            _ => unreachable!(),
        }
    }
}

impl<'b, T, const N: usize> ErrorType for Connection<'b, T, N>
where
    T: ErrorType,
{
    type Error = Error<T::Error>;
}

impl<'b, T, const N: usize> Read for Connection<'b, T, N>
where
    T: Read + Write,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.request_mut()?.io.read(buf).await
    }
}

impl<'b, T, const N: usize> Write for Connection<'b, T, N>
where
    T: Read + Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.response_mut()?.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.response_mut()?.flush().await
    }
}

struct TransitionState(());

struct RequestState<'b, T, const N: usize> {
    request: RequestHeaders<'b, N>,
    io: Body<'b, T>,
}

type ResponseState<T> = SendBody<T>;

pub trait Handler<'b, T, const N: usize>
where
    T: Read + Write,
{
    type Error: Debug;

    async fn handle(&self, connection: &mut Connection<'b, T, N>) -> Result<(), Self::Error>;
}

impl<'b, const N: usize, T, H> Handler<'b, T, N> for &H
where
    T: Read + Write,
    H: Handler<'b, T, N>,
{
    type Error = H::Error;

    async fn handle(&self, connection: &mut Connection<'b, T, N>) -> Result<(), Self::Error> {
        (**self).handle(connection).await
    }
}

pub trait TaskHandler<'b, T, const N: usize>
where
    T: Read + Write,
{
    type Error: Debug;

    async fn handle(
        &self,
        task_id: usize,
        connection: &mut Connection<'b, T, N>,
    ) -> Result<(), Self::Error>;
}

impl<'b, const N: usize, T, H> TaskHandler<'b, T, N> for &H
where
    T: Read + Write,
    H: TaskHandler<'b, T, N>,
{
    type Error = H::Error;

    async fn handle(
        &self,
        task_id: usize,
        connection: &mut Connection<'b, T, N>,
    ) -> Result<(), Self::Error> {
        (**self).handle(task_id, connection).await
    }
}

pub struct TaskHandlerAdaptor<H>(H);

impl<H> TaskHandlerAdaptor<H> {
    pub const fn new(handler: H) -> Self {
        Self(handler)
    }
}

impl<H> From<H> for TaskHandlerAdaptor<H> {
    fn from(value: H) -> Self {
        TaskHandlerAdaptor(value)
    }
}

impl<'b, const N: usize, T, H> TaskHandler<'b, T, N> for TaskHandlerAdaptor<H>
where
    T: Read + Write,
    H: Handler<'b, T, N>,
{
    type Error = H::Error;

    async fn handle(
        &self,
        _task_id: usize,
        connection: &mut Connection<'b, T, N>,
    ) -> Result<(), Self::Error> {
        self.0.handle(connection).await
    }
}

pub async fn handle_connection<const N: usize, T, H>(
    io: T,
    buf: &mut [u8],
    timeout_ms: Option<u32>,
    handler: H,
) where
    H: for<'b> Handler<'b, &'b mut T, N>,
    T: Read + Write,
{
    handle_task_connection(io, buf, timeout_ms, 0, TaskHandlerAdaptor::new(handler)).await
}

pub async fn handle_task_connection<const N: usize, T, H>(
    mut io: T,
    buf: &mut [u8],
    timeout_ms: Option<u32>,
    task_id: usize,
    handler: H,
) where
    H: for<'b> TaskHandler<'b, &'b mut T, N>,
    T: Read + Write,
{
    loop {
        debug!("Handler task {task_id}: Waiting for new request");

        let result =
            handle_task_request::<N, _, _>(buf, &mut io, task_id, timeout_ms, &handler).await;

        match result {
            Err(HandleRequestError::Connection(Error::Timeout)) => {
                info!("Handler task {task_id}: Connection closed due to timeout");
                break;
            }
            Err(HandleRequestError::Connection(Error::ConnectionClosed)) => {
                debug!("Handler task {task_id}: Connection closed");
                break;
            }
            Err(e) => {
                warn!("Handler task {task_id}: Error when handling request: {e:?}");
                break;
            }
            Ok(needs_close) => {
                if needs_close {
                    debug!("Handler task {task_id}: Request complete; closing connection");
                    break;
                } else {
                    debug!("Handler task {task_id}: Request complete");
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum HandleRequestError<C, E> {
    Connection(Error<C>),
    Handler(E),
}

impl<T, E> From<Error<T>> for HandleRequestError<T, E> {
    fn from(e: Error<T>) -> Self {
        Self::Connection(e)
    }
}

impl<C, E> fmt::Display for HandleRequestError<C, E>
where
    C: fmt::Display,
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(e) => write!(f, "Connection error: {}", e),
            Self::Handler(e) => write!(f, "Handler error: {}", e),
        }
    }
}

#[cfg(feature = "std")]
impl<C, E> std::error::Error for HandleRequestError<C, E>
where
    C: std::error::Error,
    E: std::error::Error,
{
}

pub async fn handle_request<'b, const N: usize, H, T>(
    buf: &'b mut [u8],
    io: T,
    timeout_ms: Option<u32>,
    handler: H,
) -> Result<bool, HandleRequestError<T::Error, H::Error>>
where
    H: Handler<'b, T, N>,
    T: Read + Write,
{
    handle_task_request(buf, io, 0, timeout_ms, TaskHandlerAdaptor::new(handler)).await
}

pub async fn handle_task_request<'b, const N: usize, H, T>(
    buf: &'b mut [u8],
    io: T,
    task_id: usize,
    timeout_ms: Option<u32>,
    handler: H,
) -> Result<bool, HandleRequestError<T::Error, H::Error>>
where
    H: TaskHandler<'b, T, N>,
    T: Read + Write,
{
    let mut connection = Connection::<_, N>::new(buf, io, timeout_ms).await?;

    let result = handler.handle(task_id, &mut connection).await;

    match result {
        Result::Ok(_) => connection.complete().await?,
        Result::Err(e) => connection
            .complete_err("INTERNAL ERROR")
            .await
            .map_err(|_| HandleRequestError::Handler(e))?,
    }

    Ok(connection.needs_close())
}

pub type DefaultServer =
    Server<{ DEFAULT_HANDLER_TASKS_COUNT }, { DEFAULT_BUF_SIZE }, { DEFAULT_MAX_HEADERS_COUNT }>;

pub type ServerBuffers<const P: usize, const B: usize> = MaybeUninit<[[u8; B]; P]>;

#[repr(transparent)]
pub struct Server<
    const P: usize = DEFAULT_HANDLER_TASKS_COUNT,
    const B: usize = DEFAULT_BUF_SIZE,
    const N: usize = DEFAULT_MAX_HEADERS_COUNT,
>(ServerBuffers<P, B>);

impl<const P: usize, const B: usize, const N: usize> Server<P, B, N> {
    #[inline(always)]
    pub const fn new() -> Self {
        Self(MaybeUninit::uninit())
    }

    #[inline(never)]
    #[cold]
    pub async fn run<A, H>(
        &mut self,
        acceptor: A,
        handler: H,
        timeout_ms: Option<u32>,
    ) -> Result<(), Error<A::Error>>
    where
        A: edge_nal::TcpAccept,
        H: for<'b, 't> Handler<'b, &'b mut A::Socket<'t>, N>,
    {
        let handler = TaskHandlerAdaptor::new(handler);

        // TODO: Figure out what is going on with the lifetimes so as to avoid this horrible code duplication

        let mutex = Mutex::<NoopRawMutex, _>::new(());
        let mut tasks = heapless::Vec::<_, P>::new();

        info!(
            "Creating {P} handler tasks, memory: {}B",
            core::mem::size_of_val(&tasks)
        );

        for index in 0..P {
            let mutex = &mutex;
            let acceptor = &acceptor;
            let task_id = index;
            let handler = &handler;
            let buf: *mut [u8; B] = &mut unsafe { self.0.assume_init_mut() }[index];

            tasks
                .push(async move {
                    loop {
                        debug!("Handler task {task_id}: Waiting for connection");

                        let io = {
                            let _guard = mutex.lock().await;

                            acceptor.accept().await.map_err(Error::Io)?.1
                        };

                        debug!("Handler task {task_id}: Got connection request");

                        handle_task_connection::<N, _, _>(
                            io,
                            unsafe { buf.as_mut() }.unwrap(),
                            timeout_ms,
                            task_id,
                            handler,
                        )
                        .await;
                    }
                })
                .map_err(|_| ())
                .unwrap();
        }

        let (result, _) = embassy_futures::select::select_slice(&mut tasks).await;

        warn!("Server processing loop quit abruptly: {result:?}");

        result
    }

    #[inline(never)]
    #[cold]
    pub async fn run_with_task_id<A, H>(
        &mut self,
        acceptor: A,
        handler: H,
        timeout_ms: Option<u32>,
    ) -> Result<(), Error<A::Error>>
    where
        A: edge_nal::TcpAccept,
        H: for<'b, 't> TaskHandler<'b, &'b mut A::Socket<'t>, N>,
    {
        let mutex = Mutex::<NoopRawMutex, _>::new(());
        let mut tasks = heapless::Vec::<_, P>::new();

        info!(
            "Creating {P} handler tasks, memory: {}B",
            core::mem::size_of_val(&tasks)
        );

        for index in 0..P {
            let mutex = &mutex;
            let acceptor = &acceptor;
            let task_id = index;
            let handler = &handler;
            let buf: *mut [u8; B] = &mut unsafe { self.0.assume_init_mut() }[index];

            tasks
                .push(async move {
                    loop {
                        debug!("Handler task {task_id}: Waiting for connection");

                        let io = {
                            let _guard = mutex.lock().await;

                            acceptor.accept().await.map_err(Error::Io)?.1
                        };

                        debug!("Handler task {task_id}: Got connection request");

                        handle_task_connection::<N, _, _>(
                            io,
                            unsafe { buf.as_mut() }.unwrap(),
                            timeout_ms,
                            task_id,
                            handler,
                        )
                        .await;
                    }
                })
                .map_err(|_| ())
                .unwrap();
        }

        let (result, _) = embassy_futures::select::select_slice(&mut tasks).await;

        warn!("Server processing loop quit abruptly: {result:?}");

        result
    }
}

impl<const P: usize, const B: usize, const N: usize> Default for Server<P, B, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use embedded_io_async::{Read, Write};

    use embedded_svc::http::server::asynch::{Connection, Headers, Query};
    use embedded_svc::utils::http::server::registration::{ChainHandler, ChainRoot};

    use crate::io::Body;
    use crate::RequestHeaders;

    use super::*;

    impl<'b, T, const N: usize> Headers for super::Connection<'b, T, N>
    where
        T: Read + Write,
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.request_ref()
                .expect("Not in request mode")
                .request
                .header(name)
        }
    }

    impl<'b, T, const N: usize> Query for super::Connection<'b, T, N>
    where
        T: Read + Write,
    {
        fn uri(&self) -> &'_ str {
            self.request_ref()
                .expect("Not in request mode")
                .request
                .uri()
        }

        fn method(&self) -> embedded_svc::http::Method {
            self.request_ref()
                .expect("Not in request mode")
                .request
                .method()
        }
    }

    impl<'b, T, const N: usize> Connection for super::Connection<'b, T, N>
    where
        T: Read + Write,
    {
        type Headers = RequestHeaders<'b, N>;

        type Read = Body<'b, T>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        fn split(&mut self) -> (&Self::Headers, &mut Self::Read) {
            super::Connection::split(self)
        }

        async fn initiate_response(
            &mut self,
            status: u16,
            message: Option<&str>,
            headers: &[(&str, &str)],
        ) -> Result<(), Self::Error> {
            super::Connection::initiate_response(self, status, message, headers).await
        }

        fn is_response_initiated(&self) -> bool {
            super::Connection::is_response_initiated(self)
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            // TODO: Needs a GAT rather than `&mut` return type
            // or `embedded-svc` fully upgraded to async traits & `embedded-io` 0.4 to re-enable
            //ServerConnection::raw_connection(self).map(EmbIo)
            panic!("Not supported")
        }
    }

    impl<'b, T, const N: usize> Handler<'b, T, N> for ChainRoot
    where
        T: Read + Write,
    {
        type Error = Error<T::Error>;

        async fn handle(
            &self,
            connection: &mut super::Connection<'b, T, N>,
        ) -> Result<(), Self::Error> {
            connection.initiate_response(404, None, &[]).await
        }
    }

    impl<'b, const N: usize, T, H, Q> Handler<'b, T, N> for ChainHandler<H, Q>
    where
        H: embedded_svc::http::server::asynch::Handler<super::Connection<'b, T, N>>,
        Q: Handler<'b, T, N>,
        Q::Error: Into<H::Error>,
        T: Read + Write,
    {
        type Error = H::Error;

        async fn handle(
            &self,
            connection: &mut super::Connection<'b, T, N>,
        ) -> Result<(), Self::Error> {
            let headers = connection.headers().ok();

            if let Some(headers) = headers {
                if headers.path.map(|path| self.path == path).unwrap_or(false)
                    && headers
                        .method
                        .map(|method| self.method == method.into())
                        .unwrap_or(false)
                {
                    return self.handler.handle(connection).await;
                }
            }

            self.next.handle(connection).await.map_err(Into::into)
        }
    }
}
