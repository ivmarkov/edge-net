use core::fmt::{self, Debug, Display, Write as _};
use core::future::Future;
use core::mem;

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;

use log::{info, warn};

use crate::asynch::http::{
    send_headers, send_headers_end, send_status, Body, BodyType, Error, Method, RequestHeaders,
    SendBody,
};

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

const COMPLETION_BUF_SIZE: usize = 64;

pub type HandlerErrorString = heapless::String<64>;

pub struct HandlerError(HandlerErrorString);

impl HandlerError {
    pub fn new(message: &str) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }

    pub fn release(self) -> HandlerErrorString {
        self.0
    }
}

impl<E> From<E> for HandlerError
where
    E: Debug,
{
    fn from(e: E) -> Self {
        let mut string: HandlerErrorString = "".into();

        if write!(&mut string, "{:?}", e).is_err() {
            string = "(Error string too big to serve)".into();
        }

        Self(string)
    }
}

impl Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

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

        let mut io = self.unbind_mut();

        let result = async {
            send_status(status, reason, &mut io).await?;
            let body_type = send_headers(headers.iter(), &mut io).await?;
            send_headers_end(&mut io).await?;

            Ok(body_type)
        }
        .await;

        match result {
            Ok(body_type) => {
                *self = Self::Response(SendBody::new(body_type, io));

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

    async fn complete_err<'a>(&'a mut self, err_str: &'a str) -> Result<(), Error<T::Error>> {
        if self.request_mut().is_ok() {
            let err_str_len: heapless::String<5> = (err_str.as_bytes().len() as u16).into();

            let headers = [
                ("Content-Length", err_str_len.as_str()),
                ("Content-Type", "text/plain"),
            ];

            self.complete_request(Some(500), Some("Internal Error"), &headers)
                .await?;
            self.response_mut()?.write_all(err_str.as_bytes()).await?;

            Ok(())
        } else {
            Err(Error::InvalidState)
        }
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

impl<'b, const N: usize, T> Io for ServerConnection<'b, N, T>
where
    T: Io,
{
    type Error = Error<T::Error>;
}

impl<'b, const N: usize, T> Read for ServerConnection<'b, N, T>
where
    T: Read + Write,
{
    type ReadFuture<'a> = impl Future<Output = Result<usize, Self::Error>> + 'a
    where Self: 'a;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move { self.request_mut()?.io.read(buf).await }
    }
}

impl<'b, const N: usize, T> Write for ServerConnection<'b, N, T>
where
    T: Read + Write,
{
    type WriteFuture<'a> = impl Future<Output = Result<usize, Self::Error>> + 'a
    where Self: 'a;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move { self.response_mut()?.write(buf).await }
    }

    type FlushFuture<'a> = impl Future<Output = Result<(), Self::Error>> + 'a
    where Self: 'a;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move { self.response_mut()?.flush().await }
    }
}

pub struct TransitionState(());

pub struct RequestState<'b, const N: usize, T> {
    request: RequestHeaders<'b, N>,
    io: Body<'b, T>,
}

pub type ResponseState<T> = SendBody<T>;

#[cfg(version("1.67"))]
pub trait Handler<'b, const N: usize, T> {
    async fn handle<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        connection: &'a mut ServerConnection<'b, N, T>,
    ) -> Result<(), HandlerError>;
}

// Does not typecheck with latest nightly 1.67
// See https://github.com/rust-lang/rust/issues/104691
#[cfg(not(version("1.67")))]
pub trait Handler<'b, const N: usize, T> {
    type HandleFuture<'a>: Future<Output = Result<(), HandlerError>>
    where
        Self: 'a,
        'b: 'a,
        T: 'a;

    fn handle<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        connection: &'a mut ServerConnection<'b, N, T>,
    ) -> Self::HandleFuture<'a>;
}

pub async fn handle_connection<const N: usize, const B: usize, T, H>(
    mut io: T,
    handler_id: usize,
    handler: &H,
) -> Error<T::Error>
where
    H: for<'b> Handler<'b, N, &'b mut T>,
    T: Read + Write,
{
    let mut buf = [0_u8; B];

    loop {
        warn!("Handler {}: Waiting for new request", handler_id);

        if let Err(e) = handle_request::<N, _, _>(&mut buf, &mut io, handler).await {
            info!(
                "Handler {}: Error when handling request: {:?}",
                handler_id, e
            );

            return e;
        }

        warn!("Handler {}: Request complete", handler_id);
    }
}

pub async fn handle_request<'b, const N: usize, H, T>(
    buf: &'b mut [u8],
    io: T,
    handler: &H,
) -> Result<(), Error<T::Error>>
where
    H: Handler<'b, N, T>,
    T: Read + Write,
{
    let mut connection = ServerConnection::<N, _>::new(buf, io).await?;

    let path = connection.headers()?.path.unwrap_or("");
    let method = connection.headers()?.method.unwrap_or(Method::Get);

    match handler.handle(path, method, &mut connection).await {
        Result::Ok(_) => {
            if connection.is_request_initiated() {
                connection
                    .complete_request(Some(200), Some("OK"), &[])
                    .await?;
            }

            if connection.is_response_initiated() {
                connection.complete_response().await?;
            }
        }
        Result::Err(e) => connection.complete_err(e.message()).await?,
    }

    Ok(())
}

pub struct Server<const N: usize, const B: usize, A, H> {
    acceptor: A,
    handler: H,
}

impl<const N: usize, const B: usize, A, H> Server<N, B, A, H>
where
    A: crate::asynch::tcp::TcpAccept,
    H: for<'b, 't> Handler<'b, N, &'b mut A::Connection<'t>>,
{
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
        warn!("Creating queue for {} requests", W);
        let channel = embassy_sync::channel::Channel::<R, _, W>::new();

        warn!("Creating {} handlers", P);
        let mut handlers = heapless::Vec::<_, P>::new();

        for index in 0..P {
            let channel = &channel;
            let handler_id = index;
            let handler = &self.handler;

            handlers
                .push(async move {
                    loop {
                        warn!("Handler {}: Waiting for connection", handler_id);

                        let io = channel.recv().await;
                        warn!("Handler {}: Got connection request", handler_id);

                        let err = handle_connection::<N, B, _, _>(io, handler_id, handler).await;

                        warn!(
                            "Handler {}: Connection closed because of error: {:?}",
                            handler_id, err
                        );
                    }
                })
                .map_err(|_| ())
                .unwrap();
        }

        let handlers = handlers
            .into_array::<P>()
            .unwrap_or_else(|_| unreachable!());

        embassy_futures::select::select3(
            quit,
            async {
                loop {
                    warn!("Acceptor: waiting for new connection");

                    match self.acceptor.accept().await.map_err(Error::Io) {
                        Ok(io) => {
                            warn!("Acceptor: got new connection");
                            channel.send(io).await;
                            warn!("Acceptor: connection sent");
                        }
                        Err(e) => {
                            warn!("Got error when accepting a new connection: {:?}", e);
                        }
                    }
                }
            },
            embassy_futures::select::select_array(handlers),
        )
        .await;

        warn!("Server processing loop quit");

        Ok(())
    }
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;

    use embedded_io::asynch::{Read, Write};

    use embedded_svc::http::server::asynch::{Connection, Headers, Query};
    use embedded_svc::utils::http::server::registration::{ChainHandler, ChainRoot};

    use crate::asynch::http::Method;
    use crate::asynch::http::{Body, RequestHeaders};

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
        T: Read + Write,
    {
        type Headers = RequestHeaders<'b, N>;

        type Read = Body<'b, T>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        type IntoResponseFuture<'a>
        = impl Future<Output = Result<(), Self::Error>> + 'a where Self: 'a;

        fn split(&mut self) -> (&Self::Headers, &mut Self::Read) {
            ServerConnection::split(self)
        }

        // fn headers(&self) -> Result<&Self::Headers, Self::Error> {
        //     ServerConnection::headers(self)
        // }

        fn initiate_response<'a>(
            &'a mut self,
            status: u16,
            message: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::IntoResponseFuture<'a> {
            async move { ServerConnection::initiate_response(self, status, message, headers).await }
        }

        fn is_response_initiated(&self) -> bool {
            ServerConnection::is_response_initiated(self)
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            ServerConnection::raw_connection(self)
        }
    }

    #[cfg(version("1.67"))]
    impl<'b, const N: usize, T> Handler<'b, N, T> for ChainRoot
    where
        T: Read + Write,
    {
        async fn handle<'a>(
            &'a self,
            _path: &'a str,
            _method: Method,
            connection: &'a mut ServerConnection<'b, N, T>,
        ) -> Result<(), HandlerError> {
            connection.initiate_response(404, None, &[]).await?;

            Ok(())
        }
    }

    #[cfg(version("1.67"))]
    impl<'b, const N: usize, T, H, Q> Handler<'b, N, T> for ChainHandler<H, Q>
    where
        H: for<'a> embedded_svc::http::server::asynch::Handler<&'a mut ServerConnection<'b, N, T>>,
        Q: Handler<'b, N, T>,
        T: Read + Write,
    {
        async fn handle<'a>(
            &'a self,
            path: &'a str,
            method: Method,
            connection: &'a mut ServerConnection<'b, N, T>,
        ) -> Result<(), HandlerError> {
            if self.path == path && self.method == method.into() {
                self.handler
                    .handle(connection)
                    .await
                    .map_err(|e| HandlerError(e.release()))
            } else {
                self.next.handle(path, method, connection).await
            }
        }
    }

    // Does not typecheck with latest nightly
    // See https://github.com/rust-lang/rust/issues/104691
    #[cfg(not(version("1.67")))]
    impl<'b, const N: usize, T> Handler<'b, N, T> for ChainRoot
    where
        T: Read + Write,
    {
        type HandleFuture<'a>
        = impl Future<Output = Result<(), HandlerError>> + 'a where Self: 'a, 'b: 'a, T: 'a;

        fn handle<'a>(
            &'a self,
            _path: &'a str,
            _method: Method,
            connection: &'a mut ServerConnection<'b, N, T>,
        ) -> Self::HandleFuture<'a> {
            async move {
                connection.initiate_response(404, None, &[]).await?;

                Ok(())
            }
        }
    }

    // Does not typecheck with latest nightly
    // See https://github.com/rust-lang/rust/issues/104691
    #[cfg(not(version("1.67")))]
    impl<'b, const N: usize, T, H, Q> Handler<'b, N, T> for ChainHandler<H, Q>
    where
        H: for<'a> embedded_svc::http::server::asynch::Handler<&'a mut ServerConnection<'b, N, T>>,
        Q: Handler<'b, N, T>,
        T: Read + Write,
    {
        type HandleFuture<'a>
        = impl Future<Output = Result<(), HandlerError>> + 'a where Self: 'a, 'b: 'a, T: 'a;

        fn handle<'a>(
            &'a self,
            path: &'a str,
            method: Method,
            connection: &'a mut ServerConnection<'b, N, T>,
        ) -> Self::HandleFuture<'a> {
            async move {
                if self.path == path && self.method == method.into() {
                    self.handler
                        .handle(connection)
                        .await
                        .map_err(|e| HandlerError(e.release()))
                } else {
                    self.next.handle(path, method, connection).await
                }
            }
        }
    }
}
