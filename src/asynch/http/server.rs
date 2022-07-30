#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::mem;

    use embedded_io::asynch::{Read, Write};
    use embedded_io::Io;

    use embedded_svc::http::headers::{content_len, content_type, ContentLenParseBuf};
    use embedded_svc::http::server::asynch::{Connection, Handler};
    use embedded_svc::http::server::HandlerResult;
    use embedded_svc::mutex::RawMutex;
    use embedded_svc::utils::asynch::mpmc::Channel;
    use embedded_svc::utils::asynch::select::{select3, select_all_hvec};
    use embedded_svc::utils::http::server::registration::{ChainHandler, ChainRoot};
    use log::trace;

    use crate::asynch::http::{
        send_headers, send_headers_end, send_status, Body, BodyType, Error, Method,
        Request as RawRequest, SendBody,
    };
    use crate::asynch::tcp::TcpAcceptor;

    const COMPLETION_BUF_SIZE: usize = 64;

    pub enum ServerConnection<'b, const N: usize, T> {
        Transition(Transition),
        Unbound(T),
        Request(Request<'b, N, T>),
        Response(Response<T>),
    }

    impl<'b, const N: usize, T> ServerConnection<'b, N, T>
    where
        T: Read + Write,
    {
        pub async fn new(
            buf: &'b mut [u8],
            mut io: T,
        ) -> Result<ServerConnection<'b, N, T>, Error<T::Error>> {
            let mut request = RawRequest::new();

            let (buf, read_len) = request.receive(buf, &mut io).await?;

            let io = Body::new(
                BodyType::from_headers(request.headers.iter()),
                buf,
                read_len,
                io,
            );

            Ok(Self::Request(Request { request, io }))
        }

        async fn complete_request<'a>(
            &'a mut self,
            status: Option<u16>,
            reason: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Result<(), Error<T::Error>> {
            let request = self.request()?;

            let mut buf = [0; COMPLETION_BUF_SIZE];
            while request.io.read(&mut buf).await? > 0 {}

            let mut io = self.unbind();

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
            self.response()?.finish().await?;

            Ok(())
        }

        async fn complete_err<'a>(&'a mut self, err_str: &'a str) -> Result<(), Error<T::Error>> {
            if self.request().is_ok() {
                let mut clbuf = ContentLenParseBuf::new();
                let headers = [
                    content_len(err_str.as_bytes().len() as u64, &mut clbuf),
                    content_type("text/plain"),
                ];

                self.complete_request(Some(500), Some("Internal Error"), &headers)
                    .await?;
                self.response()?.write_all(err_str.as_bytes()).await?;

                Ok(())
            } else {
                Err(Error::InvalidState)
            }
        }

        fn unbind(&mut self) -> T {
            let state = mem::replace(self, Self::Transition(Transition(())));

            match state {
                Self::Request(request) => request.io.release(),
                Self::Response(response) => response.release(),
                _ => unreachable!(),
            }
        }

        fn request(&mut self) -> Result<&mut Request<'b, N, T>, Error<T::Error>> {
            if let Self::Request(request) = self {
                Ok(request)
            } else {
                Err(Error::InvalidState)
            }
        }

        fn request_ref(&self) -> Result<&Request<'b, N, T>, Error<T::Error>> {
            if let Self::Request(request) = self {
                Ok(request)
            } else {
                Err(Error::InvalidState)
            }
        }

        fn response(&mut self) -> Result<&mut SendBody<T>, Error<T::Error>> {
            if let Self::Response(response) = self {
                Ok(response)
            } else {
                Err(Error::InvalidState)
            }
        }

        fn raw_io(&mut self) -> &mut T {
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

    impl<'b, const N: usize, T> Connection for ServerConnection<'b, N, T>
    where
        T: Read + Write,
    {
        type Headers = RawRequest<'b, N>;

        type Read = Body<'b, T>;

        type Write = SendBody<T>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        type IntoResponseFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        fn request(&mut self) -> Result<(&Self::Headers, &mut Self::Read), Self::Error> {
            self.request().map(|req| (&req.request, &mut req.io))
        }

        fn headers(&self) -> Result<&Self::Headers, Self::Error> {
            Ok(&self.request_ref()?.request)
        }

        fn initiate_response<'a>(
            &'a mut self,
            status: u16,
            message: Option<&'a str>,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::IntoResponseFuture<'a> {
            async move { self.complete_request(Some(status), message, headers).await }
        }

        fn response(&mut self) -> Result<&mut Self::Write, Self::Error> {
            self.response()
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            Ok(self.raw_io())
        }
    }

    pub struct Transition(());

    pub struct Request<'b, const N: usize, T> {
        request: RawRequest<'b, N>,
        io: Body<'b, T>,
    }

    pub type Response<T> = SendBody<T>;

    pub trait GlobalHandler<C>
    where
        C: Connection,
    {
        type HandleFuture<'a>: Future<Output = HandlerResult>
        where
            Self: 'a,
            C: 'a;

        fn handle<'a>(
            &'a self,
            path: &'a str,
            method: embedded_svc::http::Method,
            connection: C,
        ) -> Self::HandleFuture<'a> {
            self.handle_chain(false, path, method, connection)
        }

        fn handle_chain<'a>(
            &'a self,
            path_registered: bool,
            path: &'a str,
            method: embedded_svc::http::Method,
            connection: C,
        ) -> Self::HandleFuture<'a>;
    }

    pub async fn handle_connection<const N: usize, const B: usize, T, H>(
        mut io: T,
        handler: &H,
    ) -> Error<T::Error>
    where
        H: for<'a, 'b> GlobalHandler<&'a mut ServerConnection<'b, N, &'b mut T>>,
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
        H: for<'a> GlobalHandler<&'a mut ServerConnection<'b, N, T>>,
        T: Read + Write,
    {
        let mut connection = ServerConnection::new(buf, io).await?;

        let path = connection.request()?.request.path.unwrap_or("");
        let result = if let Some(method) = connection.request()?.request.method {
            handler.handle(path, method.into(), &mut connection).await
        } else {
            ChainRoot
                .handle(path, Method::Get.into(), &mut connection)
                .await
        };

        match result {
            Result::Ok(_) => {
                if connection.request().is_ok() {
                    connection
                        .complete_request(Some(200), Some("OK"), &[])
                        .await?;
                }

                if connection.response().is_ok() {
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
        A: TcpAcceptor,
        H: for<'a, 't, 'b> GlobalHandler<
            &'a mut ServerConnection<'b, N, &'b mut <A as TcpAcceptor>::Connection<'t>>,
        >,
    {
        pub const fn new(acceptor: A, handler: H) -> Self {
            Self { acceptor, handler }
        }

        pub async fn process<
            const P: usize,
            const W: usize,
            R: RawMutex,
            Q: Future<Output = ()>,
        >(
            &mut self,
            quit: Q,
        ) -> Result<(), Error<A::Error>> {
            let channel = Channel::<R, _, W>::new();
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

            select3(
                quit,
                async {
                    loop {
                        let io = self.acceptor.accept().await.map_err(Error::Io).unwrap();

                        channel.send(io).await;
                    }
                },
                select_all_hvec(handlers),
            )
            .await;

            Ok(())
        }
    }

    impl<C> GlobalHandler<C> for ChainRoot
    where
        C: Connection,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
            C: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle_chain<'a>(
            &'a self,
            path_registered: bool,
            _path: &'a str,
            _method: embedded_svc::http::Method,
            mut connection: C,
        ) -> Self::HandleFuture<'a> {
            async move {
                connection
                    .initiate_response(if path_registered { 405 } else { 404 }, None, &[])
                    .await?;

                Ok(())
            }
        }
    }

    impl<C, H, N> GlobalHandler<C> for ChainHandler<H, N>
    where
        C: Connection,
        H: Handler<C>,
        N: GlobalHandler<C>,
    {
        type HandleFuture<'a>
        where
            Self: 'a,
            C: 'a,
        = impl Future<Output = HandlerResult>;

        fn handle_chain<'a>(
            &'a self,
            path_registered: bool,
            path: &'a str,
            method: embedded_svc::http::Method,
            connection: C,
        ) -> Self::HandleFuture<'a> {
            async move {
                if self.path == path && self.method == method {
                    self.handler.handle(connection).await
                } else {
                    self.next
                        .handle_chain(path_registered, path, method, connection)
                        .await
                }
            }
        }
    }
}
