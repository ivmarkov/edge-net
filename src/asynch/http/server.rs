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
    let (read_len, headers_len) = match receive_headers::<N, _>(&mut input, buf, true).await {
        Ok(read_len) => read_len,
        Err(e) => return Err((input, e)),
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
        Err(e) => return Err((input, e.into())),
    };

    if let Status::Complete(headers_len2) = status {
        if headers_len != headers_len2 {
            panic!("Should not happen. HTTP header parsing is indeterminate.")
        }

        request.version = parser.version;
        request.method = parser.method;
        request.path = parser.path;

        trace!("Received:\n{}", request);

        let body = Body::new(&request.headers, body_buf, read_len, input);

        Ok((request, body))
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
    use core::marker::PhantomData;

    use embedded_io::{
        asynch::{Read, Write},
        Io,
    };
    use embedded_svc::http::server::asynch::{
        Completion, Handler, HandlerResult, Headers, Method, Query, Request, RequestBody,
        RequestId, Response, ResponseHeaders, ResponseWrite, SendHeaders, SendStatus, Status,
    };

    use crate::asynch::http::completion::{BodyCompletionTracker, SendBodyCompletionTracker};
    use crate::asynch::http::Error;

    use crate::close::Close;

    pub trait HandlerMatcher {
        fn matches(&self, path: &str, method: Method) -> bool;
    }

    pub trait HandlerRegistration<R> {
        type HandleFuture<'a>: Future<Output = HandlerResult>
        where
            Self: 'a;

        fn handle<'a>(
            &'a self,
            path: &'a str,
            method: Method,
            request: R,
        ) -> Self::HandleFuture<'a>;
    }

    pub struct SimpleHandlerMatcher {
        path: &'static str,
        method: Method,
    }

    impl HandlerMatcher for SimpleHandlerMatcher {
        fn matches(&self, path: &str, method: Method) -> bool {
            self.method == method && self.path == path
        }
    }

    pub struct CompositeHandlerRegistration<M, H1, H2> {
        handler1_matcher: M,
        handler1: H1,
        handler2: H2,
    }

    pub struct PrefixedHandlerMatcher<M> {
        prefix: &'static str,
        matcher: M,
    }

    impl<M> HandlerMatcher for PrefixedHandlerMatcher<M>
    where
        M: HandlerMatcher,
    {
        fn matches(&self, path: &str, method: Method) -> bool {
            self.prefix.len() < path.len()
                && self.prefix == &path[..self.prefix.len()]
                && self.matcher.matches(&path[self.prefix.len()..], method)
        }
    }

    impl<M, H, N, R> HandlerRegistration<R> for CompositeHandlerRegistration<M, H, N>
    where
        R: Request,
        M: HandlerMatcher,
        H: Handler<R>,
        N: HandlerRegistration<R>,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle<'a>(
            &'a self,
            path: &'a str,
            method: Method,
            request: R,
        ) -> Self::HandleFuture<'a> {
            async move {
                if self.handler1_matcher.matches(path, method) {
                    self.handler1.handle(request).await
                } else {
                    self.handler2.handle(path, method, request).await
                }
            }
        }
    }

    pub struct ServerRequest<'b, const N: usize, R>
    where
        R: Close,
    {
        request: super::Request<'b, N>,
        response_headers: crate::asynch::http::SendHeaders<'b>,
        io: BodyCompletionTracker<'b, R>,
    }

    impl<'b, const N: usize, R> RequestId for ServerRequest<'b, N, R>
    where
        R: Close,
    {
        fn get_request_id(&self) -> &'_ str {
            todo!()
        }
    }

    impl<'b, const N: usize, R> Headers for ServerRequest<'b, N, R>
    where
        R: Close,
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.request.header(name)
        }
    }

    impl<'b, const N: usize, R> Query for ServerRequest<'b, N, R>
    where
        R: Close,
    {
        fn query(&self) -> &'_ str {
            todo!()
        }
    }

    impl<'b, const N: usize, R> Io for ServerRequest<'b, N, R>
    where
        R: Io + Close,
    {
        type Error = Error<R::Error>;
    }

    impl<'b, const N: usize, R> Read for ServerRequest<'b, N, R>
    where
        R: Read + Close,
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
        R: Read + Write + Close,
    {
        type Headers = super::Request<'b, N>;
        type Body = ServerRequestRead<'b, N, R>;

        type Response = ServerResponse<'b, R>;
        type ResponseHeaders = ServerResponseHeaders<'b, N, R>;

        type IntoResponseFuture = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn split(self) -> (Self::Headers, Self::Body, Self::ResponseHeaders)
        where
            Self: Sized,
        {
            (
                self.request,
                ServerRequestRead(self.io),
                ServerResponseHeaders(self.response_headers, PhantomData),
            )
        }

        fn into_response(self) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                let io = SendBodyCompletionTracker::new(&self.response_headers, self.io.release());

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

    impl<'b, const N: usize> Status for super::Request<'b, N> {
        fn status(&self) -> u16 {
            todo!()
        }

        fn status_message(&self) -> Option<&'_ str> {
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
            super::Request::header(self, name)
        }
    }

    pub struct ServerRequestRead<'b, const N: usize, R>(BodyCompletionTracker<'b, R>)
    where
        R: Close;

    impl<'b, const N: usize, R> Io for ServerRequestRead<'b, N, R>
    where
        R: Io + Close,
    {
        type Error = Error<R::Error>;
    }

    impl<'b, const N: usize, R> Read for ServerRequestRead<'b, N, R>
    where
        R: Read + Close,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move { self.0.read(buf).await }
        }
    }

    impl<'b, const N: usize, R> RequestBody for ServerRequestRead<'b, N, R>
    where
        R: Read + Write + Close,
    {
        type Request = ServerRequest<'b, N, R>;

        fn merge(
            self,
            request_headers: <<Self as RequestBody>::Request as Request>::Headers,
            response_headers: <<Self as RequestBody>::Request as Request>::ResponseHeaders,
        ) -> Self::Request
        where
            Self: Sized,
        {
            todo!()
        }
    }

    pub struct ServerResponseHeaders<'b, const N: usize, R>(
        crate::asynch::http::SendHeaders<'b>,
        PhantomData<fn() -> R>,
    );

    impl<'b, const N: usize, R> SendStatus for ServerResponseHeaders<'b, N, R> {
        fn set_status(&mut self, status: u16) -> &mut Self {
            todo!()
        }

        fn set_status_message(&mut self, message: &str) -> &mut Self {
            todo!()
        }
    }

    impl<'b, const N: usize, R> SendHeaders for ServerResponseHeaders<'b, N, R> {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            todo!()
        }
    }

    impl<'b, const N: usize, R> ResponseHeaders for ServerResponseHeaders<'b, N, R>
    where
        R: Read + Write + Close,
    {
        type Request = ServerRequest<'b, N, R>;

        type IntoResponseFuture = impl Future<
            Output = Result<
                <<Self as ResponseHeaders>::Request as Request>::Response,
                <<Self as ResponseHeaders>::Request as Io>::Error,
            >,
        >;

        fn into_response(
            self,
            request_body: <<Self as ResponseHeaders>::Request as Request>::Body,
        ) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                let io = SendBodyCompletionTracker::new(&self.0, request_body.0.release());

                Ok(ServerResponse {
                    headers: self.0,
                    io,
                })
            }
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
            todo!()
        }

        fn set_status_message(&mut self, message: &str) -> &mut Self {
            todo!()
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
        W: Write + Close,
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
        W: Write + Close,
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

    pub struct Server<R, const N: usize, T>
    where
        R: for<'b> HandlerRegistration<ServerRequest<'b, N, T>>,
    {
        registration: R,
        _t: T,
    }

    impl<R, const N: usize, T> Server<R, N, T>
    where
        R: for<'b> HandlerRegistration<ServerRequest<'b, N, T>>,
        T: Read + Write + Close,
    {
        // pub fn new() -> Self {
        //     Server::<PageNotFoundHandlerRegistration, _, _>
        // }

        pub fn handle<H>(
            self,
            uri: &str,
            method: Method,
            handler: H,
        ) -> Result<
            Server<CompositeHandlerRegistration<SimpleHandlerMatcher, H, R>, N, T>,
            Error<T::Error>,
        >
        where
            H: for<'b> Handler<ServerRequest<'b, N, T>> + 'static,
        {
            //self.set_handler(uri, method, FnHandler::new(handler))
            todo!()
        }
    }

    pub struct PageNotFoundHandlerRegistration;

    impl<R> HandlerRegistration<R> for PageNotFoundHandlerRegistration
    where
        R: Request,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle(&self, uri: &str, method: Method, request: R) -> Self::HandleFuture<'_> {
            async move {
                Ok(request
                    .into_response()
                    .await?
                    .status(404)
                    .complete()
                    .await?)
            }
        }
    }
}
