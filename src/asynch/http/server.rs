#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::mem;

    use embedded_io::asynch::{Read, Write};
    use embedded_io::Io;

    use embedded_svc::http::headers::{content_len, content_type, ContentLenParseBuf};
    use embedded_svc::http::server::asynch::Connection;
    use embedded_svc::utils::http::server::registration::asynch::{
        HandlerRegistration, ServerHandler,
    };

    use crate::asynch::http::{
        send_headers, send_headers_end, send_status, Body, BodyType, Error, Method, Request,
        SendBody,
    };
    use crate::asynch::tcp::TcpAcceptor;
    use crate::close::{Close, CloseFn};

    struct PrivateData;

    pub struct ServerRequest(PrivateData);

    pub struct ServerResponse(PrivateData);

    pub enum ServerConnection<'b, const N: usize, T> {
        RequestState(Option<ServerRequestState<'b, N, T>>),
        ResponseState(Option<SendBody<T>>),
    }

    pub struct ServerRequestState<'b, const N: usize, T> {
        request: Request<'b, N>,
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
            let mut raw_request = Request::new();

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
                _ => panic!(),
            }
        }

        fn request_mut(&mut self) -> &mut ServerRequestState<'b, N, T> {
            match self {
                Self::RequestState(request) => request.as_mut().unwrap(),
                _ => panic!(),
            }
        }

        fn response_write(&mut self) -> &mut SendBody<T> {
            match self {
                Self::ResponseState(response_write) => response_write.as_mut().unwrap(),
                _ => panic!(),
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
                    let request = mem::replace(request, None).unwrap();

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

    impl<'b, const N: usize, T> Connection for ServerConnection<'b, N, T>
    where
        T: Read + Write,
    {
        type Request = ServerRequest;

        type Response = ServerResponse;

        type Headers = Request<'b, N>;

        type Read = Body<'b, T>;

        type Write = SendBody<T>;

        type IntoResponseFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn split<'a>(
            &'a mut self,
            _request: &'a mut Self::Request,
        ) -> (&'a Self::Headers, &'a mut Self::Read) {
            let req = self.request_mut();

            (&req.request, &mut req.io)
        }

        fn headers<'a>(&'a self, _request: &'a Self::Request) -> &'a Self::Headers {
            &self.request().request
        }

        fn into_response<'a>(
            &'a mut self,
            _request: Self::Request,
            status: u16,
            message: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::IntoResponseFuture<'a> {
            async move {
                let mut buf = [0_u8; 1024]; // TODO
                self.complete_request(&mut buf, Some(status), message, headers)
                    .await?;

                Ok(ServerResponse(PrivateData))
            }
        }

        fn writer<'a>(&'a mut self, _response: &'a mut Self::Response) -> &'a mut Self::Write {
            self.response_write()
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

    pub async fn test<T>(io: T)
    where
        T: Read + Write,
    {
        handle_connection::<64, 2048, _, _>(io, &ServerHandler::new())
            .await
            .unwrap();
    }

    pub async fn handle_connection<const N: usize, const B: usize, T, H>(
        mut io: T,
        handler: &ServerHandler<H>,
    ) -> Result<(), Error<T::Error>>
    where
        H: for<'b> HandlerRegistration<ServerConnection<'b, N, &'b mut T>>,
        T: Read + Write,
    {
        let mut buf = [0_u8; B];

        loop {
            handle_request::<N, _, _>(&mut buf, &mut io, &handler).await?;
        }
    }

    pub async fn handle_request<'b, const N: usize, H, T>(
        buf: &'b mut [u8],
        io: T,
        handler: &ServerHandler<H>,
    ) -> Result<(), Error<T::Error>>
    where
        H: HandlerRegistration<ServerConnection<'b, N, T>>,
        T: Read + Write,
    {
        let mut connection = ServerConnection::new(buf, io).await?;

        let path = ""; // TODO connection.request().request().request.path.unwrap_or("");
        let result = if let Some(method) = connection.request().request.method {
            handler
                .handle(
                    path,
                    method.into(),
                    &mut connection,
                    ServerRequest(PrivateData),
                )
                .await
        } else {
            ().handle(
                true,
                path,
                Method::Get.into(),
                &mut connection,
                ServerRequest(PrivateData),
            )
            .await
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
}
