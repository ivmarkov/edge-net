use core::fmt::{self, Debug};
use core::mem::{self, MaybeUninit};
use core::pin::pin;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embedded_io_async::{ErrorType, Read, Write};

use log::{debug, info, warn};

use crate::ws::{upgrade_response_headers, MAX_BASE64_KEY_RESPONSE_LEN};
use crate::DEFAULT_MAX_HEADERS_COUNT;

const DEFAULT_HANDLERS_COUNT: usize = 4;
const DEFAULT_BUF_SIZE: usize = 2048;

use super::{
    send_headers, send_headers_end, send_status, Body, BodyType, Error, RequestHeaders, SendBody,
};

#[allow(unused_imports)]
#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

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
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub async fn new(
        buf: &'b mut [u8],
        mut io: T,
    ) -> Result<Connection<'b, T, N>, Error<T::Error>> {
        let mut request = RequestHeaders::new();

        let (buf, read_len) = request.receive(buf, &mut io).await?;

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

    pub async fn initiate_ws_upgrade_response(&mut self) -> Result<(), Error<T::Error>> {
        let mut sec_key_response_base64_buf = [0_u8; MAX_BASE64_KEY_RESPONSE_LEN];

        let headers = upgrade_response_headers(
            self.headers()?.headers.iter(),
            None,
            &mut sec_key_response_base64_buf,
        )?;

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
    T: ErrorType,
{
    type Error: Debug;

    async fn handle(&self, connection: &mut Connection<'b, T, N>) -> Result<(), Self::Error>;
}

impl<'b, const N: usize, T, H> Handler<'b, T, N> for &H
where
    H: Handler<'b, T, N>,
    T: Read + Write,
{
    type Error = H::Error;

    async fn handle(&self, connection: &mut Connection<'b, T, N>) -> Result<(), Self::Error> {
        (**self).handle(connection).await
    }
}

pub async fn handle_connection<const N: usize, T, H>(
    mut io: T,
    buf: &mut [u8],
    handler_id: usize,
    handler: &H,
) where
    H: for<'b> Handler<'b, &'b mut T, N>,
    T: Read + Write,
{
    loop {
        debug!("Handler {}: Waiting for new request", handler_id);

        let result = handle_request::<N, _, _>(buf, &mut io, handler).await;

        match result {
            Err(e) => {
                warn!(
                    "Handler {}: Error when handling request: {:?}",
                    handler_id, e
                );

                break;
            }
            Ok(needs_close) => {
                if needs_close {
                    debug!(
                        "Handler {}: Request complete; closing connection",
                        handler_id
                    );
                    break;
                } else {
                    debug!("Handler {}: Request complete", handler_id);
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
    handler: &H,
) -> Result<bool, HandleRequestError<T::Error, H::Error>>
where
    H: Handler<'b, T, N>,
    T: Read + Write,
{
    let mut connection = Connection::<_, N>::new(buf, io).await?;

    let result = handler.handle(&mut connection).await;

    match result {
        Result::Ok(_) => connection.complete().await?,
        Result::Err(e) => connection
            .complete_err("INTERNAL ERROR")
            .await
            .map_err(|_| HandleRequestError::Handler(e))?,
    }

    Ok(connection.needs_close())
}

pub type DefaultServerBuffers = ServerBuffers<DEFAULT_HANDLERS_COUNT, DEFAULT_BUF_SIZE>;

pub struct ServerBuffers<const P: usize = DEFAULT_HANDLERS_COUNT, const B: usize = DEFAULT_BUF_SIZE>(
    [MaybeUninit<[u8; B]>; P],
);

impl<const P: usize, const B: usize> ServerBuffers<P, B> {
    pub const HANDLERS_COUNT: usize = P;
    pub const BUF_SIZE: usize = B;

    #[inline(always)]
    pub const fn new() -> Self {
        Self([MaybeUninit::uninit(); P])
    }
}

pub struct Server<A, H, const N: usize = DEFAULT_MAX_HEADERS_COUNT> {
    acceptor: A,
    handler: H,
}

impl<A, H, const N: usize> Server<A, H, N>
where
    A: embedded_nal_async_xtra::TcpAccept,
    H: for<'b, 't> Handler<'b, &'b mut A::Connection<'t>, N>,
{
    #[inline(always)]
    pub const fn new(acceptor: A, handler: H) -> Self {
        Self { acceptor, handler }
    }

    pub async fn process<const W: usize, const P: usize, const B: usize>(
        &mut self,
        bufs: &mut ServerBuffers<P, B>,
    ) -> Result<(), Error<A::Error>> {
        info!("Creating queue for {W} requests");
        let channel = embassy_sync::channel::Channel::<NoopRawMutex, _, W>::new();

        debug!("Creating {P} handlers");
        let mut handlers = heapless::Vec::<_, P>::new();

        for index in 0..P {
            let channel = &channel;
            let handler_id = index;
            let handler = &self.handler;
            let buf = bufs.0[index].as_mut_ptr();

            handlers
                .push(async move {
                    loop {
                        debug!("Handler {}: Waiting for connection", handler_id);

                        let io = channel.receive().await;
                        debug!("Handler {}: Got connection request", handler_id);

                        handle_connection::<N, _, _>(
                            io,
                            unsafe { buf.as_mut() }.unwrap(),
                            handler_id,
                            handler,
                        )
                        .await;
                    }
                })
                .map_err(|_| ())
                .unwrap();
        }

        let mut accept = pin!(async {
            loop {
                debug!("Acceptor: waiting for new connection");

                match self.acceptor.accept().await.map_err(Error::Io) {
                    Ok(io) => {
                        debug!("Acceptor: got new connection");
                        channel.send(io).await;
                        debug!("Acceptor: connection sent");
                    }
                    Err(e) => {
                        debug!("Got error when accepting a new connection: {:?}", e);
                    }
                }
            }
        });

        embassy_futures::select::select(
            &mut accept,
            embassy_futures::select::select_slice(&mut handlers),
        )
        .await;

        info!("Server processing loop quit");

        Ok(())
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
        T: Read + Write + 'b,
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
        T: Read + Write + 'b,
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
