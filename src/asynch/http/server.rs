use core::{fmt::Display, str};

use embedded_io::asynch::Read;

use httparse::Status;

use log::trace;

use super::*;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[allow(clippy::needless_lifetimes)]
pub async fn receive<'b, const N: usize, R>(
    buf: &'b mut [u8],
    mut input: R,
) -> Result<(Request<'b, N>, Body<'b, super::PartiallyRead<'b, R>>), (R, Error<R::Error>)>
where
    R: Read,
{
    match receive_headers(buf, &mut input).await {
        Ok((request, body_buf, read_len)) => {
            let body = Body::new(&request.headers, body_buf, read_len, input);
            Ok((request, body))
        }
        Err(e) => Err((input, e)),
    }
}

#[allow(clippy::needless_lifetimes)]
pub async fn receive_headers<'b, const N: usize, R>(
    buf: &'b mut [u8],
    mut input: R,
) -> Result<(Request<'b, N>, &'b mut [u8], usize), Error<R::Error>>
where
    R: Read,
{
    let (read_len, headers_len) = match load_headers::<N, _>(&mut input, buf, true).await {
        Ok(read_len) => read_len,
        Err(e) => return Err(e),
    };

    let mut request = Request {
        version: None,
        method: None,
        path: None,
        headers: Headers::new(),
    };

    let mut parser = httparse::Request::new(&mut request.headers.0);

    let (headers_buf, body_buf) = buf.split_at_mut(headers_len);

    let status = match parser.parse(headers_buf) {
        Ok(status) => status,
        Err(e) => return Err(e.into()),
    };

    if let Status::Complete(headers_len2) = status {
        if headers_len != headers_len2 {
            panic!("Should not happen. HTTP header parsing is indeterminate.")
        }

        request.version = parser.version;
        request.method = parser.method;
        request.path = parser.path;

        trace!("Received:\n{}", request);

        Ok((request, body_buf, read_len))
    } else {
        panic!("Secondary parse of already loaded buffer failed.")
    }
}

#[derive(Debug)]
pub struct Request<'b, const N: usize> {
    pub version: Option<u8>,
    pub method: Option<&'b str>,
    pub path: Option<&'b str>,
    pub headers: Headers<'b, N>,
}

impl<'b, const N: usize> Display for Request<'b, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(version) = self.version {
            writeln!(f, "Version {}", version)?;
        }

        if let Some(method) = self.method {
            writeln!(f, "{} {}", method, self.path.unwrap_or(""))?;
        }

        for (name, value) in self.headers.headers() {
            if name.is_empty() {
                break;
            }

            writeln!(f, "{}: {}", name, value)?;
        }

        Ok(())
    }
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;

    use embedded_io::asynch::{Read, Write};
    use embedded_io::Io;

    use embedded_svc::http::server::asynch::{
        Completion, Handler, HandlerResult, Headers, Method, Query, Request, RequestId, Response,
        ResponseWrite, SendHeaders, SendStatus,
    };

    use crate::asynch::http::completion::{
        BodyCompletionTracker, Complete, CompletionState, CompletionTracker,
        SendBodyCompletionTracker,
    };
    use crate::asynch::http::Error;
    use crate::asynch::tcp::TcpAcceptor;
    use crate::close::{Close, CloseFn};

    pub struct ServerRequest<'b, const N: usize, R> {
        request: &'b super::Request<'b, N>,
        response_headers: crate::asynch::http::SendHeaders<'b>,
        io: BodyCompletionTracker<'b, R>,
    }

    impl<'b, const N: usize, R> ServerRequest<'b, N, R>
    where
        R: Read + Write + Close + Complete,
    {
        fn new(
            request: &'b super::Request<'b, N>,
            body_buf: &'b mut [u8],
            read_len: usize,
            response_buf: &'b mut [u8],
            input: R,
        ) -> ServerRequest<'b, N, R> {
            let io = BodyCompletionTracker::new(&request.headers, body_buf, read_len, input);

            Self {
                request,
                response_headers: crate::asynch::http::SendHeaders::new(
                    response_buf,
                    &["200", "OK"],
                ),
                io,
            }
        }
    }

    impl<'b, const N: usize, R> RequestId for ServerRequest<'b, N, R> {
        fn get_request_id(&self) -> &'_ str {
            todo!()
        }
    }

    impl<'b, const N: usize, R> Headers for ServerRequest<'b, N, R> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.request.header(name)
        }
    }

    impl<'b, const N: usize, R> Query for ServerRequest<'b, N, R> {
        fn query(&self) -> &'_ str {
            todo!()
        }
    }

    impl<'b, const N: usize, R> Io for ServerRequest<'b, N, R>
    where
        R: Io,
    {
        type Error = Error<R::Error>;
    }

    impl<'b, const N: usize, R> Read for ServerRequest<'b, N, R>
    where
        R: Read + Close + Complete,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move { Ok(self.io.read(buf).await?) }
        }
    }

    impl<'b, const N: usize, R> Request for ServerRequest<'b, N, R>
    where
        R: Read + Write + Close + Complete,
    {
        type Headers<'a>
        where
            Self: 'a,
        = &'a super::Request<'b, N>;
        type Body<'a>
        where
            Self: 'a,
        = &'a mut BodyCompletionTracker<'b, R>;

        type Response = ServerResponse<'b, R>;
        type ResponseHeaders<'a>
        where
            Self: 'a,
        = &'a mut crate::asynch::http::SendHeaders<'b>;

        type IntoResponseFuture = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn split<'a>(
            &'a mut self,
        ) -> (Self::Headers<'a>, Self::Body<'a>, Self::ResponseHeaders<'a>) {
            (&self.request, &mut self.io, &mut self.response_headers)
        }

        fn into_response(self) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                let io = SendBodyCompletionTracker::new(
                    &self.response_headers,
                    self.io.release().release().release(),
                );

                Ok(ServerResponse {
                    headers: self.response_headers,
                    io,
                })
            }
        }
    }

    impl<'b, const N: usize> RequestId for super::Request<'b, N> {
        fn get_request_id(&self) -> &'_ str {
            todo!()
        }
    }

    impl<'b, const N: usize> Query for super::Request<'b, N> {
        fn query(&self) -> &'_ str {
            todo!()
        }
    }

    impl<'b, const N: usize> Headers for super::Request<'b, N> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.headers.header(name)
        }
    }

    pub struct ServerResponse<'b, W>
    where
        W: Close,
    {
        headers: crate::asynch::http::SendHeaders<'b>,
        io: SendBodyCompletionTracker<W>,
    }

    impl<'b, W> Io for ServerResponse<'b, W>
    where
        W: Io + Close,
    {
        type Error = Error<W::Error>;
    }

    impl<'b, W> SendStatus for ServerResponse<'b, W>
    where
        W: Close,
    {
        fn set_status(&mut self, status: u16) -> &mut Self {
            self.headers.set_status(status);
            self
        }

        fn set_status_message(&mut self, message: &str) -> &mut Self {
            self.headers.set_status_message(message);
            self
        }
    }

    impl<'b, W> SendHeaders for ServerResponse<'b, W>
    where
        W: Close,
    {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.headers.set_header(name, value);
            self
        }
    }

    impl<'b, W> Response for ServerResponse<'b, W>
    where
        W: Write + Close + Complete,
    {
        type Write = SendBodyCompletionTracker<W>;

        type IntoWriterFuture = impl Future<Output = Result<Self::Write, Self::Error>>;
        type SubmitFuture<'a> = impl Future<Output = Result<Completion, Self::Error>>;
        type CompleteFuture = impl Future<Output = Result<Completion, Self::Error>>;

        fn into_writer(mut self) -> Self::IntoWriterFuture
        where
            Self: Sized,
        {
            async move {
                self.io.write_all(self.headers.payload()).await?;

                Ok(self.io)
            }
        }

        fn submit<'a>(self, data: &'a [u8]) -> Self::SubmitFuture<'a>
        where
            Self: Sized,
        {
            async move {
                let mut writer = self.into_writer().await?;

                writer.write_all(data).await?;

                writer.complete().await
            }
        }

        fn complete(self) -> Self::CompleteFuture
        where
            Self: Sized,
        {
            async move { self.into_writer().await?.complete().await }
        }
    }

    impl<W> ResponseWrite for SendBodyCompletionTracker<W>
    where
        W: Write + Close + Complete,
    {
        type CompleteFuture = impl Future<Output = Result<Completion, Self::Error>>;

        fn complete(self) -> Self::CompleteFuture
        where
            Self: Sized,
        {
            async move {
                Ok(unsafe { Completion::internal_new() }) // TODO
            }
        }
    }

    pub trait HandlerRegistration<R>
    where
        R: Request,
    {
        type HandleFuture<'a>: Future<Output = HandlerResult>
        where
            Self: 'a;

        fn handle<'a>(
            &'a self,
            path_registered: bool,
            path: &'a str,
            method: Method,
            request: R,
        ) -> Self::HandleFuture<'a>;
    }

    impl<R> HandlerRegistration<R> for ()
    where
        R: Request,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle<'a>(
            &'a self,
            path_registered: bool,
            _path: &'a str,
            _method: Method,
            request: R,
        ) -> Self::HandleFuture<'a> {
            async move {
                Ok(request
                    .into_response()
                    .await?
                    .status(if path_registered { 405 } else { 404 })
                    .complete()
                    .await?)
            }
        }
    }

    pub struct SimpleHandlerRegistration<H, N> {
        path: &'static str,
        method: Method,
        handler: H,
        next: N,
    }

    impl<H, R, N> HandlerRegistration<R> for SimpleHandlerRegistration<H, N>
    where
        H: Handler<R>,
        N: HandlerRegistration<R>,
        R: Request,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle<'a>(
            &'a self,
            path_registered: bool,
            path: &'a str,
            method: Method,
            request: R,
        ) -> Self::HandleFuture<'a> {
            async move {
                let path_registered2 = if self.path == path {
                    if self.method == method {
                        return self.handler.handle(request).await;
                    }

                    true
                } else {
                    false
                };

                self.next
                    .handle(path_registered || path_registered2, path, method, request)
                    .await
            }
        }
    }

    pub struct ServerAcceptor<const N: usize, A>(A);

    impl<'t, const N: usize, A> ServerAcceptor<N, A>
    where
        A: TcpAcceptor<'t>,
    {
        pub fn new(acceptor: A) -> Self {
            Self(acceptor)
        }
    }

    impl<'t, const N: usize, A> ServerAcceptor<N, A>
    where
        A: TcpAcceptor<'t>,
    {
        pub async fn accept(
            &mut self,
        ) -> Result<<A as TcpAcceptor<'t>>::Connection<'t>, Error<A::Error>> {
            self.0.accept().await.map_err(Error::Io)
        }
    }

    pub struct ServerHandler<R, const N: usize, T>
    where
        R: for<'b> HandlerRegistration<
            ServerRequest<'b, N, &'b mut CompletionTracker<CloseFn<T, ()>>>,
        >,
    {
        registration: R,
        connection: CompletionTracker<CloseFn<T, ()>>,
    }

    impl<const N: usize, T> ServerHandler<(), N, T>
    where
        T: Read + Write,
    {
        pub fn new(connection: T) -> Self {
            Self {
                registration: (),
                connection: CompletionTracker::new(CloseFn::noop(connection)),
            }
        }
    }

    impl<R, const N: usize, T> ServerHandler<R, N, T>
    where
        R: for<'b> HandlerRegistration<
            ServerRequest<'b, N, &'b mut CompletionTracker<CloseFn<T, ()>>>,
        >,
        T: Read + Write,
    {
        pub fn handle<H>(
            self,
            path: &'static str,
            method: Method,
            handler: H,
        ) -> Result<ServerHandler<SimpleHandlerRegistration<H, R>, N, T>, Error<T::Error>>
        where
            H: for<'b> Handler<ServerRequest<'b, N, &'b mut CompletionTracker<CloseFn<T, ()>>>>
                + 'static,
        {
            Ok(ServerHandler {
                registration: SimpleHandlerRegistration {
                    path,
                    method,
                    handler,
                    next: self.registration,
                },
                connection: self.connection,
            })
        }

        pub async fn process(&mut self, buf: &mut [u8]) -> Result<(), Error<T::Error>> {
            loop {
                self.process_request(buf).await?;
            }
        }

        async fn process_request(&mut self, buf: &mut [u8]) -> Result<(), Error<T::Error>> {
            let (request_buf, response_buf) = buf.split_at_mut(buf.len() / 2);

            let (raw_request, body_buf, read_len) =
                super::receive_headers(request_buf, &mut self.connection.as_raw()).await?;
            let method = crate::asynch::http::Method::new(raw_request.method.unwrap_or("GET"));

            self.connection.reset();

            let request = ServerRequest::new(
                &raw_request,
                body_buf,
                read_len,
                response_buf,
                &mut self.connection,
            );
            if let Some(method) = method {
                let result = self
                    .registration
                    .handle(
                        false,
                        request.request.path.unwrap_or(""),
                        method.into(),
                        request,
                    )
                    .await;

                match result {
                    Result::Ok(_) => Ok(()),
                    Result::Err(e) => {
                        let (read_state, write_state) = self.connection.completion();

                        if write_state == CompletionState::NotStarted
                            && read_state == CompletionState::NotStarted
                        {
                            let request = ServerRequest::new(
                                &raw_request,
                                body_buf,
                                read_len,
                                response_buf,
                                &mut self.connection,
                            );

                            request
                                .into_response()
                                .await?
                                .status(500)
                                .status_message(e.message())
                                .complete()
                                .await?;

                            Ok(())
                        } else {
                            Err(Error::IncompleteBody)
                        }
                    }
                }
            } else {
                ().handle(true, raw_request.path.unwrap_or(""), Method::Get, request)
                    .await
                    .map_err(|_| Error::InvalidBody)?;

                Ok(())
            }
        }
    }
}
