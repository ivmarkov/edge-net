use core::fmt::{self, Debug};
use core::mem::{self, MaybeUninit};

use edge_nal::{with_timeout, Close, TcpShutdown, WithTimeout, WithTimeoutError};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;

use embedded_io_async::{ErrorType, Read, Write};

use log::{debug, info, warn};

use super::{send_headers, send_status, Body, Error, RequestHeaders, SendBody};

use crate::ws::{upgrade_response_headers, MAX_BASE64_KEY_RESPONSE_LEN};
use crate::{ConnectionType, DEFAULT_MAX_HEADERS_COUNT};

#[allow(unused_imports)]
#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

pub const DEFAULT_HANDLER_TASKS_COUNT: usize = 4;
pub const DEFAULT_BUF_SIZE: usize = 2048;
pub const DEFAULT_REQUEST_TIMEOUT_MS: u32 = 30 * 60 * 1000; // 30 minutes
pub const DEFAULT_IO_TIMEOUT_MS: u32 = 50 * 1000; // 50 seconds

const COMPLETION_BUF_SIZE: usize = 64;

/// A connection state machine for handling HTTP server requests-response cycles.
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
    /// Create a new connection state machine for an incoming request
    ///
    /// Note that the connection does not have any built-in read/write timeouts:
    /// - To add a timeout on each IO operation, wrap the `io` type with the `edge_nal::WithTimeout` wrapper.
    /// - To add a global request-response timeout, wrap your complete request-response processing
    ///   logic with the `edge_nal::with_timeout` function.
    ///
    /// Parameters:
    /// - `buf`: A buffer to store the request headers
    /// - `io`: A socket stream
    pub async fn new(
        buf: &'b mut [u8],
        mut io: T,
    ) -> Result<Connection<'b, T, N>, Error<T::Error>> {
        let mut request = RequestHeaders::new();

        let (buf, read_len) = request.receive(buf, &mut io, true).await?;

        let (connection_type, body_type) = request.resolve::<T::Error>()?;

        let io = Body::new(body_type, buf, read_len, io);

        Ok(Self::Request(RequestState {
            request,
            io,
            connection_type,
        }))
    }

    /// Return `true` of the connection is in request state (i.e. the initial state upon calling `new`)
    pub fn is_request_initiated(&self) -> bool {
        matches!(self, Self::Request(_))
    }

    /// Split the connection into request headers and body
    pub fn split(&mut self) -> (&RequestHeaders<'b, N>, &mut Body<'b, T>) {
        let req = self.request_mut().expect("Not in request mode");

        (&req.request, &mut req.io)
    }

    /// Return a reference to the request headers
    pub fn headers(&self) -> Result<&RequestHeaders<'b, N>, Error<T::Error>> {
        Ok(&self.request_ref()?.request)
    }

    /// Return `true` if the request is a WebSocket upgrade request
    pub fn is_ws_upgrade_request(&self) -> Result<bool, Error<T::Error>> {
        Ok(self.headers()?.is_ws_upgrade_request())
    }

    /// Switch the connection into a response state
    ///
    /// Parameters:
    /// - `status`: The HTTP status code
    /// - `message`: An optional HTTP status message
    /// - `headers`: An array of HTTP response headers.
    ///   Note that if no `Content-Length` or `Transfer-Encoding` headers are provided,
    ///   the body will be send with chunked encoding (for HTTP1.1 only and if the connection is not Close)
    pub async fn initiate_response(
        &mut self,
        status: u16,
        message: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<(), Error<T::Error>> {
        self.complete_request(Some(status), message, headers).await
    }

    /// A convenience method to initiate a WebSocket upgrade response
    pub async fn initiate_ws_upgrade_response(
        &mut self,
        buf: &mut [u8; MAX_BASE64_KEY_RESPONSE_LEN],
    ) -> Result<(), Error<T::Error>> {
        let headers = upgrade_response_headers(self.headers()?.headers.iter(), None, buf)?;

        self.initiate_response(101, None, &headers).await
    }

    /// Return `true` if the connection is in response state
    pub fn is_response_initiated(&self) -> bool {
        matches!(self, Self::Response(_))
    }

    /// Completes the response and switches the connection back to the unbound state
    /// If the connection is still in a request state, and empty 200 OK response is sent
    pub async fn complete(&mut self) -> Result<(), Error<T::Error>> {
        if self.is_request_initiated() {
            self.complete_request(Some(200), Some("OK"), &[]).await?;
        }

        if self.is_response_initiated() {
            self.complete_response().await?;
        }

        Ok(())
    }

    /// Completes the response with an error message and switches the connection back to the unbound state
    ///
    /// If the connection is still in a request state, an empty 500 Internal Error response is sent
    pub async fn complete_err(&mut self, err: &str) -> Result<(), Error<T::Error>> {
        let result = self.request_mut();

        match result {
            Ok(_) => {
                let headers = [("Connection", "Close"), ("Content-Type", "text/plain")];

                self.complete_request(Some(500), Some("Internal Error"), &headers)
                    .await?;

                let response = self.response_mut()?;

                response.io.write_all(err.as_bytes()).await?;
                response.io.finish().await?;

                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Return `true` if the connection needs to be closed
    ///
    /// This is determined by the connection type (i.e. `Connection: Close` header)
    pub fn needs_close(&self) -> bool {
        match self {
            Self::Response(response) => response.needs_close(),
            _ => true,
        }
    }

    /// Switch the connection to unbound state, returning a mutable reference to the underlying socket stream
    ///
    /// NOTE: Use with care, and only if the connection is completed in the meantime
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
        let request_connection_type = request.connection_type;

        let mut io = self.unbind_mut();

        let result = async {
            send_status(http11, status, reason, &mut io).await?;

            let (connection_type, body_type) = send_headers(
                headers.iter(),
                Some(request_connection_type),
                false,
                http11,
                true,
                &mut io,
            )
            .await?;

            Ok((connection_type, body_type))
        }
        .await;

        match result {
            Ok((connection_type, body_type)) => {
                *self = Self::Response(ResponseState {
                    io: SendBody::new(body_type, io),
                    connection_type,
                });

                Ok(())
            }
            Err(e) => {
                *self = Self::Unbound(io);

                Err(e)
            }
        }
    }

    async fn complete_response(&mut self) -> Result<(), Error<T::Error>> {
        self.response_mut()?.io.finish().await?;

        Ok(())
    }

    fn unbind_mut(&mut self) -> T {
        let state = mem::replace(self, Self::Transition(TransitionState(())));

        match state {
            Self::Request(request) => request.io.release(),
            Self::Response(response) => response.io.release(),
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

    fn response_mut(&mut self) -> Result<&mut ResponseState<T>, Error<T::Error>> {
        if let Self::Response(response) = self {
            Ok(response)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn io_mut(&mut self) -> &mut T {
        match self {
            Self::Request(request) => request.io.as_raw_reader(),
            Self::Response(response) => response.io.as_raw_writer(),
            Self::Unbound(io) => io,
            _ => unreachable!(),
        }
    }
}

impl<T, const N: usize> ErrorType for Connection<'_, T, N>
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
        self.response_mut()?.io.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.response_mut()?.io.flush().await
    }
}

struct TransitionState(());

struct RequestState<'b, T, const N: usize> {
    request: RequestHeaders<'b, N>,
    io: Body<'b, T>,
    connection_type: ConnectionType,
}

struct ResponseState<T> {
    io: SendBody<T>,
    connection_type: ConnectionType,
}

impl<T> ResponseState<T>
where
    T: Write,
{
    fn needs_close(&self) -> bool {
        matches!(self.connection_type, ConnectionType::Close) || self.io.needs_close()
    }
}

/// A trait (async callback) for handling incoming HTTP requests
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

/// A trait (async callback) for handling a single HTTP request
///
/// The only difference between this and `Handler` is that this trait has an additional `task_id` parameter,
/// which is used for logging purposes
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

/// A type that adapts a `Handler` into a `TaskHandler`
pub struct TaskHandlerAdaptor<H>(H);

impl<H> TaskHandlerAdaptor<H> {
    /// Create a new `TaskHandlerAdaptor` from a `Handler`
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

/// A convenience function to handle multiple HTTP requests over a single socket stream,
/// using the specified handler.
///
/// The socket stream will be closed only in case of error, or until the client explicitly requests that
/// either with a hard socket close, or with a `Connection: Close` header.
///
/// Parameters:
/// - `io`: A socket stream
/// - `buf`: A work-area buffer used by the implementation
/// - `request_timeout_ms`: An optional timeout for a complete request-response processing, in milliseconds.
///   If not provided, a default timeout of 30 minutes is used.
/// - `handler`: An implementation of `Handler` to handle incoming requests
pub async fn handle_connection<const N: usize, T, H>(
    io: T,
    buf: &mut [u8],
    request_timeout_ms: Option<u32>,
    handler: H,
) where
    H: for<'b> Handler<'b, &'b mut T, N>,
    T: Read + Write + TcpShutdown,
{
    handle_task_connection(
        io,
        buf,
        request_timeout_ms,
        0,
        TaskHandlerAdaptor::new(handler),
    )
    .await
}

/// A convenience function to handle multiple HTTP requests over a single socket stream,
/// using the specified task handler.
///
/// The socket stream will be closed only in case of error, or until the client explicitly requests that
/// either with a hard socket close, or with a `Connection: Close` header.
///
/// Parameters:
/// - `io`: A socket stream
/// - `buf`: A work-area buffer used by the implementation
/// - `request_timeout_ms`: An optional timeout for a complete request-response processing, in milliseconds.
///   If not provided, a default timeout of 30 minutes is used.
/// - `task_id`: An identifier for the task, used for logging purposes
/// - `handler`: An implementation of `TaskHandler` to handle incoming requests
pub async fn handle_task_connection<const N: usize, T, H>(
    mut io: T,
    buf: &mut [u8],
    request_timeout_ms: Option<u32>,
    task_id: usize,
    handler: H,
) where
    H: for<'b> TaskHandler<'b, &'b mut T, N>,
    T: Read + Write + TcpShutdown,
{
    let close = loop {
        debug!("Handler task {task_id}: Waiting for new request");

        let result = with_timeout(
            request_timeout_ms.unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS),
            handle_task_request::<N, _, _>(buf, &mut io, task_id, &handler),
        )
        .await;

        match result {
            Err(WithTimeoutError::Timeout) => {
                info!("Handler task {task_id}: Connection closed due to request timeout");
                break false;
            }
            Err(WithTimeoutError::IO(HandleRequestError::Connection(Error::ConnectionClosed))) => {
                debug!("Handler task {task_id}: Connection closed");
                break false;
            }
            Err(e) => {
                warn!("Handler task {task_id}: Error when handling request: {e:?}");
                break true;
            }
            Ok(needs_close) => {
                if needs_close {
                    debug!("Handler task {task_id}: Request complete; closing connection");
                    break true;
                } else {
                    debug!("Handler task {task_id}: Request complete");
                }
            }
        }
    };

    if close {
        if let Err(e) = io.close(Close::Both).await {
            warn!("Handler task {task_id}: Error when closing the socket: {e:?}");
        }
    } else {
        let _ = io.abort().await;
    }
}

/// The error type for handling HTTP requests
#[derive(Debug)]
pub enum HandleRequestError<C, E> {
    /// A connection error (HTTP protocol error or a socket IO error)
    Connection(Error<C>),
    /// A handler error
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

impl<C, E> embedded_io_async::Error for HandleRequestError<C, E>
where
    C: Debug + embedded_io_async::Error,
    E: Debug,
{
    fn kind(&self) -> embedded_io_async::ErrorKind {
        match self {
            Self::Connection(Error::Io(e)) => e.kind(),
            _ => embedded_io_async::ErrorKind::Other,
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

/// A convenience function to handle a single HTTP request over a socket stream,
/// using the specified handler.
///
/// Note that this function does not set any timeouts on the request-response processing
/// or on the IO operations. It is up that the caller to use the `with_timeout` function
/// and the `WithTimeout` struct from the `edge-nal` crate to wrap the future returned
/// by this function, or the socket stream, or both.
///
/// Parameters:
/// - `buf`: A work-area buffer used by the implementation
/// - `io`: A socket stream
/// - `handler`: An implementation of `Handler` to handle incoming requests
pub async fn handle_request<'b, const N: usize, H, T>(
    buf: &'b mut [u8],
    io: T,
    handler: H,
) -> Result<bool, HandleRequestError<T::Error, H::Error>>
where
    H: Handler<'b, T, N>,
    T: Read + Write,
{
    handle_task_request(buf, io, 0, TaskHandlerAdaptor::new(handler)).await
}

/// A convenience function to handle a single HTTP request over a socket stream,
/// using the specified task handler.
///
/// Note that this function does not set any timeouts on the request-response processing
/// or on the IO operations. It is up that the caller to use the `with_timeout` function
/// and the `WithTimeout` struct from the `edge-nal` crate to wrap the future returned
/// by this function, or the socket stream, or both.
///
/// Parameters:
/// - `buf`: A work-area buffer used by the implementation
/// - `io`: A socket stream
/// - `task_id`: An identifier for the task, used for logging purposes
/// - `handler`: An implementation of `TaskHandler` to handle incoming requests
pub async fn handle_task_request<'b, const N: usize, H, T>(
    buf: &'b mut [u8],
    io: T,
    task_id: usize,
    handler: H,
) -> Result<bool, HandleRequestError<T::Error, H::Error>>
where
    H: TaskHandler<'b, T, N>,
    T: Read + Write,
{
    let mut connection = Connection::<_, N>::new(buf, io).await?;

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

/// A type alias for an HTTP server with default buffer sizes.
pub type DefaultServer =
    Server<{ DEFAULT_HANDLER_TASKS_COUNT }, { DEFAULT_BUF_SIZE }, { DEFAULT_MAX_HEADERS_COUNT }>;

/// A type alias for the HTTP server buffers (essentially, arrays of `MaybeUninit`)
pub type ServerBuffers<const P: usize, const B: usize> = MaybeUninit<[[u8; B]; P]>;

/// An HTTP server that can handle multiple requests concurrently.
///
/// The server needs an implementation of `edge_nal::TcpAccept` to accept incoming connections.
#[repr(transparent)]
pub struct Server<
    const P: usize = DEFAULT_HANDLER_TASKS_COUNT,
    const B: usize = DEFAULT_BUF_SIZE,
    const N: usize = DEFAULT_MAX_HEADERS_COUNT,
>(ServerBuffers<P, B>);

impl<const P: usize, const B: usize, const N: usize> Server<P, B, N> {
    /// Create a new HTTP server
    #[inline(always)]
    pub const fn new() -> Self {
        Self(MaybeUninit::uninit())
    }

    /// Run the server with the specified acceptor and handler
    ///
    /// Parameters:
    /// - `acceptor`: An implementation of `edge_nal::TcpAccept` to accept incoming connections
    /// - `handler`: An implementation of `Handler` to handle incoming requests
    /// - `request_timeout_ms`: An optional timeout for a complete request-response processing, in milliseconds.
    ///   If not provided, a default timeout of 30 minutes is used.
    /// - `io_timeout_ms`: An optional timeout for each IO operation, in milliseconds.
    ///   If not provided, a default timeout of 50 seconds is used.
    #[inline(never)]
    #[cold]
    pub async fn run<A, H>(
        &mut self,
        acceptor: A,
        handler: H,
        request_timeout_ms: Option<u32>,
        io_timeout_ms: Option<u32>,
    ) -> Result<(), Error<A::Error>>
    where
        A: edge_nal::TcpAccept,
        H: for<'b, 't> Handler<'b, &'b mut WithTimeout<A::Socket<'t>>, N>,
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

                        let io =
                            WithTimeout::new(io_timeout_ms.unwrap_or(DEFAULT_IO_TIMEOUT_MS), io);

                        debug!("Handler task {task_id}: Got connection request");

                        handle_task_connection::<N, _, _>(
                            io,
                            unsafe { buf.as_mut() }.unwrap(),
                            request_timeout_ms,
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

    /// Run the server with the specified acceptor and task handler
    ///
    /// Parameters:
    /// - `acceptor`: An implementation of `edge_nal::TcpAccept` to accept incoming connections
    /// - `handler`: An implementation of `TaskHandler` to handle incoming requests
    /// - `request_timeout_ms`: An optional timeout for a complete request-response processing, in milliseconds.
    ///   If not provided, a default timeout of 30 minutes is used.
    /// - `io_timeout_ms`: An optional timeout for each IO operation, in milliseconds.
    ///   If not provided, a default timeout of 50 seconds is used.
    #[inline(never)]
    #[cold]
    pub async fn run_with_task_id<A, H>(
        &mut self,
        acceptor: A,
        handler: H,
        request_timeout_ms: Option<u32>,
        io_timeout_ms: Option<u32>,
    ) -> Result<(), Error<A::Error>>
    where
        A: edge_nal::TcpAccept,
        H: for<'b, 't> TaskHandler<'b, &'b mut WithTimeout<A::Socket<'t>>, N>,
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

                        let io =
                            WithTimeout::new(io_timeout_ms.unwrap_or(DEFAULT_IO_TIMEOUT_MS), io);

                        debug!("Handler task {task_id}: Got connection request");

                        handle_task_connection::<N, _, _>(
                            io,
                            unsafe { buf.as_mut() }.unwrap(),
                            request_timeout_ms,
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
