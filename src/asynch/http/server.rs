#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::{pending, Future};

    use embedded_io::asynch::{Read, Write};
    use embedded_io::Io;

    use embedded_svc::http::headers::{content_len, content_type, ContentLenParseBuf};
    use embedded_svc::http::server::asynch::{Handler, Request};
    use embedded_svc::http::server::{FnHandler, HandlerResult};
    use embedded_svc::http::Headers;
    use embedded_svc::mutex::RawMutex;
    use embedded_svc::utils::asynch::mpmc::Channel;
    use embedded_svc::utils::asynch::select::{select3, select_all_hvec};
    use embedded_svc::utils::http::server::registration::asynch::{
        HandlerChain, RootHandlerChainBuilder,
    };

    use crate::asynch::http::{
        send_headers, send_headers_end, send_status, Body, BodyType, Error, Method,
        Request as RawRequest, SendBody,
    };
    use crate::asynch::tcp::TcpAcceptor;

    //////

    // pub struct Request1<T>(T);

    // pub trait Handler<T>
    // where
    //     T: Read,
    // {
    //     type HandleFuture<'a>: Future<Output = ()> where Self: 'a;

    //     fn handle(&self, request: Request1<T>) -> Self::HandleFuture<'_>;
    // }

    // impl<T> Handler<T> for ()
    // where T: Read,
    // {
    //     type HandleFuture<'a> where Self: 'a = impl Future<Output = ()>;

    //     fn handle(&self, request: Request1<T>) -> Self::HandleFuture<'_> {
    //         async move {

    //         }
    //     }
    // }
    // pub async fn test0() {
    //     let conn = conn().await;

    //     test(conn, ()).await;
    // }

    // pub async fn test<T, H>(mut connection: T, handler: H)
    // where
    //     H: for <'a> Handler<&'a mut T>,
    //     T: Read + 'static,
    // {
    //     loop {
    //         handler.handle(Request1(&mut connection)).await;

    //         handler.handle(Request1(&mut connection)).await;
    //     }
    // }

    // pub async fn conn() -> impl Read {
    // }

    ///////

    pub async fn test(request: impl Request) -> HandlerResult {
        let content_type = request.headers().content_type().unwrap();

        request
            .into_response(200, None, &[("Content-Type", "zzz")])
            .await?;

        Ok(())
    }

    pub struct Q;

    impl<R> Handler<R> for Q
    where
        R: Request,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
            R: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle<'a>(&'a self, request: R) -> Self::HandleFuture<'a> {
            test(request)
        }
    }

    pub enum ServerConnection<'b, const N: usize, T> {
        RequestState(Option<ServerRequestState<'b, N, T>>),
        ResponseState(Option<SendBody<T>>),
    }

    pub struct ServerRequestState<'b, const N: usize, T> {
        request: RawRequest<'b, N>,
        io: Body<'b, T>,
    }

    impl<'b, const N: usize, T> ServerConnection<'b, N, T> {
        pub async fn new(
            buf: &'b mut [u8],
            mut io: T,
        ) -> Result<ServerConnection<'b, N, T>, Error<T::Error>>
        where
            T: Read + Write,
        {
            let mut raw_request = RawRequest::new();

            let (buf, read_len) = raw_request.receive(buf, &mut io).await?;

            let body = Body::new(
                BodyType::from_headers(raw_request.headers.iter()),
                buf,
                read_len,
                io,
            );

            Ok(Self::RequestState(Some(ServerRequestState {
                request: raw_request,
                io: body,
            })))
        }

        fn request(&self) -> &ServerRequestState<'b, N, T> {
            match self {
                Self::RequestState(request) => request.as_ref().unwrap(),
                _ => unreachable!(),
            }
        }

        fn request_mut(&mut self) -> &mut ServerRequestState<'b, N, T> {
            match self {
                Self::RequestState(request) => request.as_mut().unwrap(),
                _ => unreachable!(),
            }
        }

        fn response_write(&mut self) -> &mut SendBody<T> {
            match self {
                Self::ResponseState(response_write) => response_write.as_mut().unwrap(),
                _ => unreachable!(),
            }
        }

        fn raw_io(&mut self) -> &mut T
        where
            T: Read + Write,
        {
            match self {
                Self::RequestState(request) => request.as_mut().unwrap().io.as_raw_reader(),
                Self::ResponseState(response_write) => {
                    response_write.as_mut().unwrap().as_raw_writer()
                }
            }
        }

        async fn complete_request<'a>(
            &'a mut self,
            buf: &'a mut [u8],
            status: Option<u16>,
            reason: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Result<Option<&'a mut SendBody<T>>, Error<T::Error>>
        where
            T: Read + Write,
        {
            match self {
                Self::RequestState(request) => {
                    let io = &mut request.as_mut().unwrap().io;

                    while io.read(buf).await? > 0 {}
                    let request = request.take().unwrap();

                    let mut io = request.io.release();

                    send_status(status, reason, &mut io).await?;
                    let body_type = send_headers(headers.iter(), &mut io).await?;
                    send_headers_end(&mut io).await?;

                    let io = SendBody::new(body_type, io);

                    *self = Self::ResponseState(Some(io));

                    Ok(Some(self.response_write()))
                }
                Self::ResponseState(_) => Ok(None),
            }
        }

        async fn complete_response<'a>(
            &'a mut self,
            buf: &'a mut [u8],
            status: Option<u16>,
            reason: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Result<bool, Error<T::Error>>
        where
            T: Read + Write,
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
            let headers = [
                content_len(err_str.as_bytes().len() as u64, &mut clbuf),
                content_type("text/plain"),
            ];

            if let Some(body) = self
                .complete_request(buf, Some(500), Some("Internal Error"), &headers)
                .await?
            {
                body.write_all(err_str.as_bytes()).await?;

                Ok(true)
            } else {
                Ok(false)
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
        T: Read,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move { self.request_mut().io.read(buf).await }
        }
    }

    impl<'b, const N: usize, T> Write for ServerConnection<'b, N, T>
    where
        T: Write,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            async move { self.response_write().write(buf).await }
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        fn flush<'a>(&'a mut self) -> Self::FlushFuture<'a> {
            async move { self.response_write().flush().await }
        }
    }

    pub type ServerRequest<'m, 'b, const N: usize, T> = &'m mut ServerConnection<'b, N, T>;

    impl<'m, 'b, const N: usize, T> Request for ServerRequest<'m, 'b, N, T>
    where
        T: Read + Write + 'm,
        'b: 'm,
    {
        type Response = ServerRequest<'m, 'b, N, T>;

        type Headers = RawRequest<'b, N>;

        type Read = Body<'b, T>;

        type Write = SendBody<T>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        type IntoResponseFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn split<'a>(&'a mut self) -> (&'a Self::Headers, &'a mut Self::Read) {
            let req = self.request_mut();

            (&req.request, &mut req.io)
        }

        fn headers<'a>(&'a self) -> &'a Self::Headers {
            &self.request().request
        }

        fn into_response<'a>(
            mut self,
            status: u16,
            message: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::IntoResponseFuture<'a>
        where
            Self: Sized + 'a,
        {
            async move {
                let mut buf = [0_u8; 1024]; // TODO
                self.complete_request(&mut buf, Some(status), message, headers)
                    .await?;

                Ok(self)
            }
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            Ok(self.raw_io())
        }
    }

    pub trait GlobalHandler<R>
    where
        R: Request,
    {
        type HandleFuture<'a>: Future<Output = HandlerResult>
        where
            Self: 'a,
            R: 'a;

        fn handle<'a>(
            &'a self,
            path: &'a str,
            method: embedded_svc::http::Method,
            request: R,
        ) -> Self::HandleFuture<'a> {
            self.handle_chain(false, path, method, request)
        }

        fn handle_chain<'a>(
            &'a self,
            path_registered: bool,
            path: &'a str,
            method: embedded_svc::http::Method,
            request: R,
        ) -> Self::HandleFuture<'a>;
    }

    pub async fn handle_connection<const N: usize, const B: usize, T>(
        mut io: T,
        //handler: &H,
    ) -> Result<(), Error<T::Error>>
    where
        //H: for<'m, 'b> GlobalHandler<ServerRequest<'m, 'b, N, &'b mut T>>,
        T: Read + Write,
    {
        let mut buf = [0_u8; B];

        let handler = ();

        loop {
            handle_request::<N, _, _>(&mut buf, &mut io, &handler).await?;
        }
    }

    pub async fn handle_request<'b, const N: usize, T, H>(
        buf: &'b mut [u8],
        io: T,
        handler: &H,
    ) -> Result<(), Error<T::Error>>
    where
        H: for<'m> GlobalHandler<ServerRequest<'m, 'b, N, T>>,
        T: Read + Write,
    {
        let mut connection = ServerConnection::<N, _>::new(buf, io).await?;

        let path = connection.request().request.path.unwrap_or("");
        let result = if let Some(method) = connection.request().request.method {
            handler.handle(path, method.into(), &mut connection).await
        } else {
            ().handle(path, Method::Get.into(), &mut connection).await
        };

        let mut buf = [0_u8; 64];

        let completed = match result {
            Result::Ok(_) => {
                connection
                    .complete_response(&mut buf, Some(200), Some("OK"), &[])
                    .await?
            }
            Result::Err(e) => connection.complete_err(&mut buf, e.message()).await?,
        };

        if completed {
            Ok(())
        } else {
            Err(Error::IncompleteBody)
        }
    }

    pub struct Server<const N: usize, const B: usize, A>(A);

    pub type ServerAcceptorRequest<'m, 't, 'b, const N: usize, A> =
        ServerRequest<'m, 'b, N, &'b mut <A as TcpAcceptor>::Connection<'t>>;

    impl<const N: usize, const B: usize, A> Server<N, B, A>
    where
        A: TcpAcceptor,
    {
        pub const fn new(acceptor: A) -> Self {
            Self(acceptor)
        }

        pub async fn process<const P: usize, const W: usize, R, Q>(
            &mut self,
            quit: Q,
        ) -> Result<(), Error<A::Error>>
        where
            R: RawMutex,
            //H: for<'m, 't, 'b> GlobalHandler<ServerAcceptorRequest<'m, 't, 'b, N, A>>,
            Q: Future<Output = ()>,
        {
            let channel = Channel::<R, _, W>::new();
            let mut handlers = heapless::Vec::<_, P>::new();

            for _ in 0..P {
                handlers
                    .push(async {
                        loop {
                            let io = channel.recv().await;

                            handle_connection::<N, B, _>(io).await.unwrap();
                        }
                    })
                    .map_err(|_| ())
                    .unwrap();
            }

            select3(
                quit,
                async {
                    loop {
                        let io = self.0.accept().await.map_err(Error::Io).unwrap();

                        channel.send(io).await;
                    }
                },
                select_all_hvec(handlers),
            )
            .await;

            Ok(())
        }
    }

    impl<R> GlobalHandler<R> for ()
    where
        R: Request,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
            R: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle_chain<'a>(
            &'a self,
            path_registered: bool,
            _path: &'a str,
            _method: embedded_svc::http::Method,
            request: R,
        ) -> Self::HandleFuture<'a> {
            async move {
                request
                    .into_status_response(if path_registered { 405 } else { 404 })
                    .await?;

                Ok(())
            }
        }
    }

    impl<H, R, N> GlobalHandler<R> for HandlerChain<H, N>
    where
        H: Handler<R>,
        N: GlobalHandler<R>,
        R: Request,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
            R: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle_chain<'a>(
            &'a self,
            path_registered: bool,
            path: &'a str,
            method: embedded_svc::http::Method,
            request: R,
        ) -> Self::HandleFuture<'a> {
            async move {
                let path_found = self.path == path;

                if self.method == method {
                    self.handler.handle(request).await
                } else {
                    self.next
                        .handle_chain(path_registered || path_found, path, method, request)
                        .await
                }
            }
        }
    }

    pub async fn h<A, R>(acceptor: A)
    where
        A: TcpAcceptor,
        R: RawMutex,
    {
        let mut server = Server::<1, 1, _>::new(acceptor);

        server.process::<1, 1, R, _>(pending()).await.unwrap();
    }
}
