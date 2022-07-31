use core::fmt::{self, Debug, Display, Write as _};
use core::future::Future;
use core::mem;

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;

use log::trace;

use crate::asynch::http::{
    send_headers, send_headers_end, send_status, Body, BodyType, Error, Method, RequestHeaders,
    SendBody,
};
use crate::asynch::tcp::TcpAcceptor;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

const COMPLETION_BUF_SIZE: usize = 64;

pub struct HandlerError(heapless::String<128>);

impl HandlerError {
    pub fn new(message: &str) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }

    pub fn release(self) -> heapless::String<128> {
        self.0
    }
}

impl<E> From<E> for HandlerError
where
    E: Debug,
{
    fn from(e: E) -> Self {
        let mut string: heapless::String<128> = "".into();

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

    pub fn split(&mut self) -> Result<(&RequestHeaders<'b, N>, &mut Body<'b, T>), Error<T::Error>> {
        self.request_mut().map(|req| (&req.request, &mut req.io))
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

    pub fn assert_response(&mut self) -> Result<(), Error<T::Error>> {
        self.response_mut()?;

        Ok(())
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
    type ReadFuture<'a> = impl Future<Output = Result<usize, Self::Error>>
    where Self: 'a;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move { self.request_mut()?.io.read(buf).await }
    }
}

impl<'b, const N: usize, T> Write for ServerConnection<'b, N, T>
where
    T: Read + Write,
{
    type WriteFuture<'a> = impl Future<Output = Result<usize, Self::Error>>
    where Self: 'a;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move { self.response_mut()?.write(buf).await }
    }

    type FlushFuture<'a> = impl Future<Output = Result<(), Self::Error>>
    where Self: 'a;

    fn flush<'a>(&'a mut self) -> Self::FlushFuture<'a> {
        async move { self.response_mut()?.flush().await }
    }
}

pub struct TransitionState(());

pub struct RequestState<'b, const N: usize, T> {
    request: RequestHeaders<'b, N>,
    io: Body<'b, T>,
}

pub type ResponseState<T> = SendBody<T>;

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
    handler: &H,
) -> Error<T::Error>
where
    H: for<'b> Handler<'b, N, &'b mut T>,
    T: Read + Write,
{
    let mut buf = [0_u8; B];

    loop {
        if let Err(e) = handle_request::<N, _, _>(&mut buf, &mut io, handler).await {
            return e;
        }
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
            if connection.split().is_ok() {
                connection
                    .complete_request(Some(200), Some("OK"), &[])
                    .await?;
            }

            if connection.assert_response().is_ok() {
                connection.complete_response().await?;
            }
        }
        Result::Err(e) => connection.complete_err(e.message()).await?,
    }

    Ok(())
}

#[cfg(feature = "embassy-util")]
pub struct Server<const N: usize, const B: usize, A, H> {
    acceptor: A,
    handler: H,
}

#[cfg(feature = "embassy-util")]
impl<const N: usize, const B: usize, A, H> Server<N, B, A, H>
where
    A: TcpAcceptor,
    H: for<'b, 't> Handler<'b, N, &'b mut A::Connection<'t>>,
{
    pub const fn new(acceptor: A, handler: H) -> Self {
        Self { acceptor, handler }
    }

    pub async fn process<
        const P: usize,
        const W: usize,
        R: embassy_util::blocking_mutex::raw::RawMutex,
        Q: Future<Output = ()>,
    >(
        &mut self,
        quit: Q,
    ) -> Result<(), Error<A::Error>> {
        let channel = embassy_util::channel::mpmc::Channel::<R, _, W>::new();
        let mut handlers = heapless::Vec::<_, P>::new();

        for _ in 0..P {
            handlers
                .push(async {
                    loop {
                        let io = channel.recv().await;

                        let err = handle_connection::<N, B, _, _>(io, &self.handler).await;

                        trace!("Connection closed because of error: {:?}", err);
                    }
                })
                .map_err(|_| ())
                .unwrap();
        }

        let handlers = handlers
            .into_array::<P>()
            .unwrap_or_else(|_| unreachable!());

        embassy_util::select3(
            quit,
            async {
                loop {
                    let io = self.acceptor.accept().await.map_err(Error::Io).unwrap();

                    channel.send(io).await;
                }
            },
            embassy_util::select_all(handlers),
        )
        .await;

        Ok(())
    }
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;

    use embedded_io::asynch::{Read, Write};

    use embedded_svc::http::server::asynch::Connection;
    use embedded_svc::utils::http::server::registration::{ChainHandler, ChainRoot};

    use crate::asynch::http::{Body, Method, RequestHeaders};

    use super::*;

    impl<'b, const N: usize, T> Connection for ServerConnection<'b, N, T>
    where
        T: Read + Write,
    {
        type Headers = RequestHeaders<'b, N>;

        type Read = Body<'b, T>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        type IntoResponseFuture<'a>
        = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

        fn split(&mut self) -> Result<(&Self::Headers, &mut Self::Read), Self::Error> {
            ServerConnection::split(self)
        }

        fn headers(&self) -> Result<&Self::Headers, Self::Error> {
            ServerConnection::headers(self)
        }

        fn initiate_response<'a>(
            &'a mut self,
            status: u16,
            message: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::IntoResponseFuture<'a> {
            async move { ServerConnection::initiate_response(self, status, message, headers).await }
        }

        fn assert_response(&mut self) -> Result<(), Self::Error> {
            ServerConnection::assert_response(self)
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            ServerConnection::raw_connection(self)
        }
    }

    impl<'b, const N: usize, T> Handler<'b, N, T> for ChainRoot
    where
        T: Read + Write,
    {
        type HandleFuture<'a>
        = impl Future<Output = Result<(), HandlerError>> where Self: 'a, 'b: 'a, T: 'a;

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

    impl<'b, const N: usize, T, H, Q> Handler<'b, N, T> for ChainHandler<H, Q>
    where
        H: for<'a> embedded_svc::http::server::asynch::Handler<&'a mut ServerConnection<'b, N, T>>,
        Q: Handler<'b, N, T>,
        T: Read + Write,
    {
        type HandleFuture<'a>
        = impl Future<Output = Result<(), HandlerError>> where Self: 'a, 'b: 'a, T: 'a;

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
