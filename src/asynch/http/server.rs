#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::{iter, mem};

    use embedded_io::asynch::{Read, Write};
    use embedded_io::Io;

    use embedded_svc::http::headers::{content_len, content_type, ContentLenParseBuf};
    use embedded_svc::http::server::asynch::{Handler, HandlerResult, Headers, Query};

    use crate::asynch::http::{
        send_headers, send_headers_end, send_status, Body, BodyType, Error, Method, Request,
        SendBody,
    };
    use crate::asynch::tcp::TcpAcceptor;
    use crate::close::{Close, CloseFn};

    pub enum ServerRequestResponseState<'b, const N: usize, T> {
        New,
        Request(Option<ServerRequestState<'b, N, T>>),
        ResponseWrite(Option<SendBody<T>>),
    }

    pub struct ServerRequestState<'b, const N: usize, T> {
        request: Request<'b, N>,
        io: Body<'b, T>,
    }

    impl<'b, const N: usize, T> ServerRequestResponseState<'b, N, T> {
        fn request(&self) -> &ServerRequestState<'b, N, T> {
            match self {
                Self::Request(request) => request.as_ref().unwrap(),
                _ => panic!(),
            }
        }

        fn request_mut(&mut self) -> &mut ServerRequestState<'b, N, T> {
            match self {
                Self::Request(request) => request.as_mut().unwrap(),
                _ => panic!(),
            }
        }

        fn response_write(&mut self) -> &mut SendBody<T> {
            match self {
                Self::ResponseWrite(response_write) => response_write.as_mut().unwrap(),
                _ => panic!(),
            }
        }

        async fn complete_request<'a, H>(
            &'a mut self,
            buf: &'a mut [u8],
            status: Option<u16>,
            reason: Option<&'a str>,
            headers: H,
        ) -> Result<Option<&'a mut SendBody<T>>, Error<T::Error>>
        where
            T: Read + Write,
            H: IntoIterator<Item = (&'a str, &'a str)>,
        {
            match self {
                Self::New => panic!(),
                Self::Request(request) => {
                    let io = &mut request.as_mut().unwrap().io;

                    while io.read(buf).await? > 0 {}
                    let request = mem::replace(request, None).unwrap();

                    let mut io = request.io.release();

                    send_status(status, reason, &mut io).await?;
                    let body_type = send_headers(headers, &mut io).await?;
                    send_headers_end(&mut io).await?;

                    let io = SendBody::new(body_type, io);

                    *self = Self::ResponseWrite(Some(io));

                    Ok(Some(self.response_write()))
                }
                Self::ResponseWrite(_) => Ok(None),
            }
        }

        async fn complete_response<'a, H>(
            &'a mut self,
            buf: &'a mut [u8],
            status: Option<u16>,
            reason: Option<&'a str>,
            headers: H,
        ) -> Result<bool, Error<T::Error>>
        where
            T: Read + Write,
            H: IntoIterator<Item = (&'a str, &'a str)>,
        {
            if let Some(body) = self.complete_request(buf, status, reason, headers).await? {
                body.finish().await?;

                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn complete_err<'a>(
            &'a mut self,
            buf: &'a mut [u8],
            err_str: &'a str,
        ) -> Result<bool, Error<T::Error>>
        where
            T: Read + Write,
        {
            let mut clbuf = ContentLenParseBuf::new();
            let headers = content_len(err_str.as_bytes().len() as u64, &mut clbuf)
                .chain(content_type("text/plain"));

            if let Some(body) = self
                .complete_request(buf, Some(500), Some("Internal Error"), headers)
                .await?
            {
                body.write_all(err_str.as_bytes()).await?;

                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    pub struct ServerRequest<'a, 'b, const N: usize, T>(
        &'a mut ServerRequestResponseState<'b, N, T>,
    );

    impl<'a, 'b, const N: usize, T> ServerRequest<'a, 'b, N, T> {
        pub async fn new(
            buf: &'b mut [u8],
            mut io: T,
            state: &'a mut ServerRequestResponseState<'b, N, T>,
        ) -> Result<ServerRequest<'a, 'b, N, T>, Error<T::Error>>
        where
            T: Read + Write,
            'b: 'a,
        {
            let mut raw_request = Request::new();

            let (buf, read_len) = raw_request.receive(buf, &mut io).await?;

            let body = Body::new(
                BodyType::from_headers(raw_request.headers.iter()),
                buf,
                read_len,
                io,
            );

            *state = ServerRequestResponseState::Request(Some(ServerRequestState {
                request: raw_request,
                io: body,
            }));

            Ok(Self(state))
        }
    }

    pub struct ServerResponseWrite<'a, 'b, const N: usize, T>(
        &'a mut ServerRequestResponseState<'b, N, T>,
    );

    impl<'a, 'b, const N: usize, R> Headers for ServerRequest<'a, 'b, N, R> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.0.request().request.header(name)
        }
    }

    impl<'a, 'b, const N: usize, R> Query for ServerRequest<'a, 'b, N, R> {
        fn query(&self) -> &'_ str {
            todo!()
        }
    }

    impl<'a, 'b, const N: usize, R> Io for ServerRequest<'a, 'b, N, R>
    where
        R: Io,
    {
        type Error = Error<R::Error>;
    }

    impl<'a, 'b, const N: usize, R> Read for ServerRequest<'a, 'b, N, R>
    where
        R: Read + 'a,
        'b: 'a,
    {
        type ReadFuture<'f>
        where
            Self: 'f,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'f>(&'f mut self, buf: &'f mut [u8]) -> Self::ReadFuture<'f> {
            async move { Ok(self.0.request_mut().io.read(buf).await?) }
        }
    }

    impl<'a, 'b, const N: usize, R> embedded_svc::http::server::asynch::Request
        for ServerRequest<'a, 'b, N, R>
    where
        'b: 'a,
        R: Read + Write + 'a,
    {
        type Headers<'f>
        where
            Self: 'f,
        = &'f Request<'b, N>;
        type Read<'f>
        where
            Self: 'f,
        = &'f mut Body<'b, R>;

        type ResponseWrite = ServerResponseWrite<'a, 'b, N, R>;

        type IntoResponseFuture<'f, H> =
            impl Future<Output = Result<Self::ResponseWrite, Self::Error>>;
        type IntoOkResponseFuture = impl Future<Output = Result<Self::ResponseWrite, Self::Error>>;

        fn split<'f>(&'f mut self) -> (Self::Headers<'f>, Self::Read<'f>) {
            let request = self.0.request_mut();

            (&request.request, &mut request.io)
        }

        fn into_response<'f, H>(
            self,
            status: u16,
            message: Option<&'f str>,
            headers: H,
        ) -> Self::IntoResponseFuture<'f, H>
        where
            H: IntoIterator<Item = (&'f str, &'f str)>,
            Self: Sized,
        {
            async move {
                let mut buf = [0_u8; 32];

                // self.0.complete_request(&mut buf, Some(status), message, headers).await?;
                self.0
                    .complete_request(&mut buf, Some(status), message, iter::empty())
                    .await?;

                Ok(ServerResponseWrite(self.0))
            }
        }

        fn into_ok_response(self) -> Self::IntoOkResponseFuture
        where
            Self: Sized,
        {
            async move {
                let mut buf = [0_u8; 32];

                self.0
                    .complete_request(&mut buf, Some(200), Some("OK"), iter::empty())
                    .await?;

                Ok(ServerResponseWrite(self.0))
            }
        }
    }

    impl<'a, 'b, const N: usize, W> Io for ServerResponseWrite<'a, 'b, N, W>
    where
        W: Write,
    {
        type Error = Error<W::Error>;
    }

    impl<'a, 'b, const N: usize, W> Write for ServerResponseWrite<'a, 'b, N, W>
    where
        'b: 'a,
        W: Write + 'a,
    {
        type WriteFuture<'f>
        where
            Self: 'f,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn write<'f>(&'f mut self, buf: &'f [u8]) -> Self::WriteFuture<'f> {
            async move { Ok(self.0.response_write().write(buf).await?) }
        }

        type FlushFuture<'f>
        where
            Self: 'f,
        = impl Future<Output = Result<(), Self::Error>>;

        fn flush<'f>(&'f mut self) -> Self::FlushFuture<'f> {
            async move { Ok(self.0.response_write().flush().await?) }
        }
    }

    ///////////////////////////////

    // pub struct ServerAcceptor<const N: usize, A>(A);

    // impl<'t, const N: usize, A> ServerAcceptor<N, A>
    // where
    //     A: TcpAcceptor<'t>,
    // {
    //     pub fn new(acceptor: A) -> Self {
    //         Self(acceptor)
    //     }
    // }

    // impl<'t, const N: usize, A> ServerAcceptor<N, A>
    // where
    //     A: TcpAcceptor<'t>,
    // {
    //     pub async fn accept(
    //         &mut self,
    //     ) -> Result<<A as TcpAcceptor<'t>>::Connection<'t>, Error<A::Error>> {
    //         self.0.accept().await.map_err(Error::Io)
    //     }
    // }

    pub trait HandlerRegistration<R>
    where
        R: embedded_svc::http::server::asynch::Request,
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
        R: embedded_svc::http::server::asynch::Request,
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
                request
                    .into_response(if path_registered { 405 } else { 404 }, None, iter::empty())
                    .await?;

                Ok(())
            }
        }
    }

    pub struct SimpleHandlerRegistration<H, N> {
        path: &'static str,
        method: Method,
        handler: H,
        next: N,
    }

    impl<H, N> SimpleHandlerRegistration<H, N> {
        const fn new(path: &'static str, method: Method, handler: H, next: N) -> Self {
            Self {
                path,
                method,
                handler,
                next,
            }
        }
    }

    impl<H, R, N> HandlerRegistration<R> for SimpleHandlerRegistration<H, N>
    where
        H: Handler<R>,
        N: HandlerRegistration<R>,
        R: embedded_svc::http::server::asynch::Request,
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

    pub struct ServerHandler<H>(H);

    impl ServerHandler<()> {
        pub fn new() -> Self {
            Self(())
        }
    }

    impl<H> ServerHandler<H> {
        pub fn register<H2, R>(
            self,
            path: &'static str,
            method: Method,
            handler: H2,
        ) -> ServerHandler<SimpleHandlerRegistration<H2, H>>
        where
            H2: Handler<R> + 'static,
            R: embedded_svc::http::server::asynch::Request,
        {
            ServerHandler(SimpleHandlerRegistration::new(
                path, method, handler, self.0,
            ))
        }

        pub async fn handle<'a, R>(
            &'a self,
            path: &'a str,
            method: Method,
            request: R,
        ) -> HandlerResult
        where
            H: HandlerRegistration<R>,
            R: embedded_svc::http::server::asynch::Request,
        {
            self.0.handle(false, path, method, request).await
        }
    }

    pub async fn process<const N: usize, H, T>(
        mut io: T,
        handler: &ServerHandler<H>,
    ) -> Result<(), Error<T::Error>>
    where
        H: for<'a, 'b> HandlerRegistration<ServerRequest<'a, 'b, N, &'b mut T>>,
        T: Read + Write,
    {
        loop {
            let mut buf = [0_u8; 1024];
            process_request(&mut buf, &mut io, &handler).await?;
        }
    }

    async fn process_request<'b, const N: usize, H, T>(
        buf: &'b mut [u8],
        io: &'b mut T,
        handler: &ServerHandler<H>,
    ) -> Result<(), Error<T::Error>>
    where
        H: for<'a> HandlerRegistration<ServerRequest<'a, 'b, N, &'b mut T>>,
        T: Read + Write,
    {
        let mut state = ServerRequestResponseState::New;

        let request = ServerRequest::new(buf, io, &mut state).await?;

        let path = request.0.request().request.path.unwrap_or("");
        let result = if let Some(method) = request.0.request().request.method {
            handler.handle(path, method, request).await
        } else {
            ().handle(true, path, Method::Get, request).await
        };

        match result {
            Result::Ok(_) => Ok(()),
            Result::Err(e) => {
                let mut buf = [0_u8; 64];

                if !state.complete_err(&mut buf, e.message()).await? {
                    Err(Error::IncompleteBody)
                } else {
                    Ok(())
                }
            }
        }
    }
}
