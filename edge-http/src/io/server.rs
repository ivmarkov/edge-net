use core::fmt::{self, Debug};
use core::future::Future;
use core::mem;
use core::pin::pin;

use embedded_io_async::{ErrorType, Read, Write};

use log::{debug, info, warn};

use super::{
    send_headers, send_headers_end, send_status, Body, BodyType, Error, Method, RequestHeaders,
    SendBody,
};

#[allow(unused_imports)]
#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

const COMPLETION_BUF_SIZE: usize = 64;

pub enum ServerConnection<'b, const N: usize, T> {
    Transition(TransitionState),
    Unbound(T),
    Request(RequestState<'b, N, T>),
    Response(ResponseState<T>),
}

impl<'b, const N: usize, T> ServerConnection<'b, N, T>
where
    T: Read + Write,
{
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub async fn new(
        buf: &'b mut [u8],
        mut io: T,
    ) -> Result<ServerConnection<'b, N, T>, Error<T::Error>> {
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

    pub async fn initiate_response<'a>(
        &'a mut self,
        status: u16,
        message: Option<&'a str>,
        headers: &'a [(&'a str, &'a str)],
    ) -> Result<(), Error<T::Error>> {
        self.complete_request(Some(status), message, headers).await
    }

    pub fn is_response_initiated(&self) -> bool {
        matches!(self, Self::Response(_))
    }

    pub async fn complete_ok(&mut self) -> Result<(), Error<T::Error>> {
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

    pub fn raw_connection(&mut self) -> Result<&mut T, Error<T::Error>> {
        Ok(self.io_mut())
    }

    async fn complete_request<'a>(
        &'a mut self,
        status: Option<u16>,
        reason: Option<&'a str>,
        headers: &'a [(&'a str, &'a str)],
    ) -> Result<(), Error<T::Error>> {
        let request = self.request_mut()?;

        let mut buf = [0; COMPLETION_BUF_SIZE];
        while request.io.read(&mut buf).await? > 0 {}

        let http11 = request.request.http11.unwrap_or(false);

        let mut io = self.unbind_mut();

        let result = async {
            send_status(http11, status, reason, &mut io).await?;
            let body_type = send_headers(
                headers.iter().filter(|(k, v)| {
                    http11
                        || !k.eq_ignore_ascii_case("Transfer-Encoding")
                        || !v.eq_ignore_ascii_case("Chunked")
                }),
                &mut io,
            )
            .await?;

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
            _ => unreachable!(),
        }
    }

    fn request_mut(&mut self) -> Result<&mut RequestState<'b, N, T>, Error<T::Error>> {
        if let Self::Request(request) = self {
            Ok(request)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn request_ref(&self) -> Result<&RequestState<'b, N, T>, Error<T::Error>> {
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

impl<'b, const N: usize, T> ErrorType for ServerConnection<'b, N, T>
where
    T: ErrorType,
{
    type Error = Error<T::Error>;
}

impl<'b, const N: usize, T> Read for ServerConnection<'b, N, T>
where
    T: Read + Write,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.request_mut()?.io.read(buf).await
    }
}

impl<'b, const N: usize, T> Write for ServerConnection<'b, N, T>
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

pub struct TransitionState(());

pub struct RequestState<'b, const N: usize, T> {
    request: RequestHeaders<'b, N>,
    io: Body<'b, T>,
}

pub type ResponseState<T> = SendBody<T>;

pub trait Handler<'b, const N: usize, T>
where
    T: ErrorType,
{
    type Error: Debug;

    async fn handle<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        connection: &'a mut ServerConnection<'b, N, T>,
    ) -> Result<(), Self::Error>;
}

pub async fn handle_connection<const N: usize, const B: usize, T, H>(
    mut io: T,
    handler_id: usize,
    handler: &H,
) where
    H: for<'b> Handler<'b, N, &'b mut T>,
    T: Read + Write,
{
    let mut buf = [0_u8; B];

    loop {
        debug!("Handler {}: Waiting for new request", handler_id);

        let result = handle_request::<N, _, _>(&mut buf, &mut io, handler).await;

        match result {
            Err(e) => {
                info!(
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
    H: Handler<'b, N, T>,
    T: Read + Write,
{
    let mut connection = ServerConnection::<N, _>::new(buf, io).await?;

    let path = connection.headers()?.path.unwrap_or("");
    let method = connection.headers()?.method.unwrap_or(Method::Get);

    let result = handler.handle(path, method, &mut connection).await;

    match result {
        Result::Ok(_) => connection.complete_ok().await?,
        Result::Err(e) => {
            warn!("Error when handling request: {e:?}");
            connection
                .complete_err("INTERNAL ERROR")
                .await
                .map_err(|_| HandleRequestError::Handler(e))?
        }
    }

    Ok(connection.needs_close())
}

pub struct Server<const N: usize, const B: usize, A, H> {
    acceptor: A,
    handler: H,
}

impl<const N: usize, const B: usize, A, H> Server<N, B, A, H>
where
    A: embedded_nal_async_xtra::TcpAccept,
    H: for<'b, 't> Handler<'b, N, &'b mut A::Connection<'t>>,
{
    #[inline(always)]
    pub const fn new(acceptor: A, handler: H) -> Self {
        Self { acceptor, handler }
    }

    pub async fn process<
        const P: usize,
        const W: usize,
        R: embassy_sync::blocking_mutex::raw::RawMutex,
        Q: Future<Output = ()>,
    >(
        &mut self,
        quit: Q,
    ) -> Result<(), Error<A::Error>> {
        let mut quit = pin!(quit);

        info!("Creating queue for {} requests", W);
        let channel = embassy_sync::channel::Channel::<R, _, W>::new();

        debug!("Creating {} handlers", P);
        let mut handlers = heapless::Vec::<_, P>::new();

        for index in 0..P {
            let channel = &channel;
            let handler_id = index;
            let handler = &self.handler;

            handlers
                .push(async move {
                    loop {
                        debug!("Handler {}: Waiting for connection", handler_id);

                        let io = channel.receive().await;
                        debug!("Handler {}: Got connection request", handler_id);

                        handle_connection::<N, B, _, _>(io, handler_id, handler).await;
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

        embassy_futures::select::select3(
            &mut quit,
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
    use crate::{Method, RequestHeaders};

    use super::*;

    impl<'b, const N: usize, T> Headers for ServerConnection<'b, N, T>
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

    impl<'b, const N: usize, T> Query for ServerConnection<'b, N, T>
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

    impl<'b, const N: usize, T> Connection for ServerConnection<'b, N, T>
    where
        T: Read + Write + 'b,
    {
        type Headers = RequestHeaders<'b, N>;

        type Read = Body<'b, T>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        fn split(&mut self) -> (&Self::Headers, &mut Self::Read) {
            ServerConnection::split(self)
        }

        // fn headers(&self) -> Result<&Self::Headers, Self::Error> {
        //     ServerConnection::headers(self)
        // }

        async fn initiate_response(
            &mut self,
            status: u16,
            message: Option<&str>,
            headers: &[(&str, &str)],
        ) -> Result<(), Self::Error> {
            ServerConnection::initiate_response(self, status, message, headers).await
        }

        fn is_response_initiated(&self) -> bool {
            ServerConnection::is_response_initiated(self)
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            // TODO: Needs a GAT rather than `&mut` return type
            // or `embedded-svc` fully upgraded to async traits & `embedded-io` 0.4 to re-enable
            //ServerConnection::raw_connection(self).map(EmbIo)
            panic!("Not supported")
        }
    }

    impl<'b, const N: usize, T> Handler<'b, N, T> for ChainRoot
    where
        T: Read + Write,
    {
        type Error = Error<T::Error>;

        async fn handle<'a>(
            &'a self,
            _path: &'a str,
            _method: Method,
            connection: &'a mut ServerConnection<'b, N, T>,
        ) -> Result<(), Self::Error> {
            connection.initiate_response(404, None, &[]).await
        }
    }

    impl<'b, const N: usize, T, H, Q> Handler<'b, N, T> for ChainHandler<H, Q>
    where
        H: embedded_svc::http::server::asynch::Handler<ServerConnection<'b, N, T>>,
        Q: Handler<'b, N, T>,
        Q::Error: Into<H::Error>,
        T: Read + Write + 'b,
    {
        type Error = H::Error;

        async fn handle<'a>(
            &'a self,
            path: &'a str,
            method: Method,
            connection: &'a mut ServerConnection<'b, N, T>,
        ) -> Result<(), Self::Error> {
            if self.path == path && self.method == method.into() {
                self.handler.handle(connection).await
            } else {
                self.next
                    .handle(path, method, connection)
                    .await
                    .map_err(Into::into)
            }
        }
    }
}
