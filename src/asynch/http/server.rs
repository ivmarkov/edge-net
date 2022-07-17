#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::{iter, mem};

    use embedded_io::asynch::{Read, Write};
    use embedded_io::Io;

    use embedded_svc::http::headers::{content_len, ContentLenParseBuf};
    use embedded_svc::http::server::asynch::{Handler, HandlerResult, Headers, Method, Query};

    use crate::asynch::http::completion::CompletionState;
    use crate::asynch::http::{
        send_headers, send_headers_end, send_status, Body, BodyType, Error, PartiallyRead, Request,
        SendBody,
    };
    use crate::asynch::tcp::TcpAcceptor;
    use crate::close::{Close, CloseFn};

    pub enum ServerRequestResponseState<'b, const N: usize, T> {
        Request(Option<ServerRequestState<'b, N, T>>),
        ResponseWrite(Option<SendBody<T>>),
    }

    pub struct ServerRequestState<'b, const N: usize, T> {
        request: Request<'b, N>,
        io: Body<'b, PartiallyRead<'b, T>>,
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
        ) -> Result<Option<&mut SendBody<T>>, Error<T::Error>>
        where
            T: Read + Write,
            H: IntoIterator<Item = (&'a str, &'a str)>,
        {
            match self {
                Self::Request(request) => {
                    let io = &mut request.as_mut().unwrap().io;

                    while io.read(buf).await? > 0 {}

                    let request = mem::replace(request, None).unwrap();

                    let io = request.io.release().release();

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
            if let Some(body) = self.complete_request(buf, status, reason, headers)? {
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
            let clbuf = ContentLenParseBuf::new();

            if let Some(body) = self
                .complete_request(
                    buf,
                    Some(500),
                    Some("Internal Error"),
                    iter::once(content_len(err_str.as_bytes.len(), &mut clbuf))
                        .chain(iter::once(("Content-Type", "text/plain"))),
                )
                .await?
            {
                body.write_all(err_str.as_bytes()).await?;

                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    // pub struct ServerHandlerRequest<'b, const N: usize, T>(
    //     &'b mut ServerRequestResponseState<'b, N, T>,
    // );

    // pub struct ServerHandlerResponse<'b, const N: usize, T>(
    //     &'b mut ServerRequestResponseState<'b, N, T>,
    // );

    // pub struct ServerHandlerResponseWrite<'b, const N: usize, T>(
    //     &'b mut ServerRequestResponseState<'b, N, T>,
    // );

    // impl<'b, const N: usize, R> Headers for ServerHandlerRequest<'b, N, R> {
    //     fn header(&self, name: &str) -> Option<&'_ str> {
    //         self.0.request().request.header(name)
    //     }
    // }

    // impl<'b, const N: usize, R> Query for ServerHandlerRequest<'b, N, R> {
    //     fn query(&self) -> &'_ str {
    //         todo!()
    //     }
    // }

    // impl<'b, const N: usize, R> Io for ServerHandlerRequest<'b, N, R>
    // where
    //     R: Io,
    // {
    //     type Error = Error<R::Error>;
    // }

    // impl<'b, const N: usize, R> Read for ServerHandlerRequest<'b, N, R>
    // where
    //     R: Read + 'b,
    // {
    //     type ReadFuture<'a>
    //     where
    //         Self: 'a,
    //     = impl Future<Output = Result<usize, Self::Error>>;

    //     fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
    //         async move { Ok(self.0.request_mut().io.read(buf).await?) }
    //     }
    // }

    // impl<'b, const N: usize, R> embedded_svc::http::server::asynch::Request for ServerHandlerRequest<'b, N, R>
    // where
    //     R: Read + Write + 'b,
    // {
    //     type Headers<'a>
    //     where
    //         Self: 'a,
    //     = &'a Request<'b, N>;
    //     type Read<'a>
    //     where
    //         Self: 'a,
    //     = &'a mut Body<'b, PartiallyRead<'b, R>>;

    //     type ResponseWrite = ServerHandlerResponse<'b, N, R>;

    //     type IntoResponseFuture<'a, H> = impl Future<Output = Result<Self::ResponseWrite, Self::Error>>;

    //     fn split<'a>(&'a mut self) -> (Self::Headers<'a>, Self::Read<'a>) {
    //         //let request = self.0.request_mut();

    //         (&request.request, &mut request.io)
    //     }

    //     fn into_response(self) -> Self::IntoResponseFuture
    //     where
    //         Self: Sized,
    //     {
    //         async move {
    //             self.0.switch_response();

    //             Ok(ServerHandlerResponse(self.0))
    //         }
    //     }
    // }

    // impl<'b, const N: usize, W> Io for ServerHandlerResponseWrite<'b, N, W>
    // where
    //     W: Write,
    // {
    //     type Error = Error<W::Error>;
    // }

    // impl<'b, const N: usize, W> Write for ServerHandlerResponseWrite<'b, N, W>
    // where
    //     W: Write + 'b,
    // {
    //     type WriteFuture<'a>
    //     where
    //         Self: 'a,
    //     = impl Future<Output = Result<usize, Self::Error>>;

    //     fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
    //         async move { Ok(self.0.response_write().io.write(buf).await?) }
    //     }

    //     type FlushFuture<'a>
    //     where
    //         Self: 'a,
    //     = impl Future<Output = Result<(), Self::Error>>;

    //     fn flush<'a>(&'a mut self) -> Self::FlushFuture<'a> {
    //         async move { Ok(self.0.response_write().io.flush().await?) }
    //     }
    // }

    ///////////////////////////////

    // pub trait HandlerRegistration<R>
    // where
    //     R: Request,
    // {
    //     type HandleFuture<'a>: Future<Output = HandlerResult>
    //     where
    //         Self: 'a;

    //     fn handle<'a>(
    //         &'a self,
    //         path_registered: bool,
    //         path: &'a str,
    //         method: Method,
    //         request: R,
    //     ) -> Self::HandleFuture<'a>;
    // }

    // impl<R> HandlerRegistration<R> for ()
    // where
    //     R: Request,
    // {
    //     type HandleFuture<'a>
    //     where
    //         Self: 'a,
    //     = impl Future<Output = HandlerResult>;

    //     fn handle<'a>(
    //         &'a self,
    //         path_registered: bool,
    //         _path: &'a str,
    //         _method: Method,
    //         request: R,
    //     ) -> Self::HandleFuture<'a> {
    //         async move {
    //             Ok(request
    //                 .into_response()
    //                 .await?
    //                 .status(if path_registered { 405 } else { 404 })
    //                 .complete()
    //                 .await?)
    //         }
    //     }
    // }

    // pub struct SimpleHandlerRegistration<H, N> {
    //     path: &'static str,
    //     method: Method,
    //     handler: H,
    //     next: N,
    // }

    // impl<H, R, N> HandlerRegistration<R> for SimpleHandlerRegistration<H, N>
    // where
    //     H: Handler<R>,
    //     N: HandlerRegistration<R>,
    //     R: Request,
    // {
    //     type HandleFuture<'a>
    //     where
    //         Self: 'a,
    //     = impl Future<Output = HandlerResult>;

    //     fn handle<'a>(
    //         &'a self,
    //         path_registered: bool,
    //         path: &'a str,
    //         method: Method,
    //         request: R,
    //     ) -> Self::HandleFuture<'a> {
    //         async move {
    //             let path_registered2 = if self.path == path {
    //                 if self.method == method {
    //                     return self.handler.handle(request).await;
    //                 }

    //                 true
    //             } else {
    //                 false
    //             };

    //             self.next
    //                 .handle(path_registered || path_registered2, path, method, request)
    //                 .await
    //         }
    //     }
    // }

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

    // pub struct ServerHandler<R, const N: usize, T>
    // where
    //     R: for<'b> HandlerRegistration<ServerHandlerRequest<'b, N, &'b mut T>>,
    // {
    //     registration: R,
    //     connection: T,
    // }

    // impl<const N: usize, T> ServerHandler<(), N, T>
    // where
    //     T: Read + Write,
    //     T: 'static,
    // {
    //     pub fn new(connection: T) -> Self {
    //         Self {
    //             registration: (),
    //             connection,
    //         }
    //     }
    // }

    // impl<R, const N: usize, T> ServerHandler<R, N, T>
    // where
    //     R: for<'b> HandlerRegistration<ServerHandlerRequest<'b, N, &'b mut T>>,
    //     T: Read + Write + 'static,
    // {
    //     pub fn handle<H>(
    //         self,
    //         path: &'static str,
    //         method: Method,
    //         handler: H,
    //     ) -> Result<ServerHandler<SimpleHandlerRegistration<H, R>, N, T>, Error<T::Error>>
    //     where
    //         H: for<'b> Handler<ServerHandlerRequest<'b, N, &'b mut T>> + 'static,
    //     {
    //         Ok(ServerHandler {
    //             registration: SimpleHandlerRegistration {
    //                 path,
    //                 method,
    //                 handler,
    //                 next: self.registration,
    //             },
    //             connection: self.connection,
    //         })
    //     }

    //     pub async fn process(&mut self, buf: &mut [u8]) -> Result<(), Error<T::Error>> {
    //         loop {
    //             self.process_request(buf).await?;
    //         }
    //     }

    //     async fn process_request(&mut self, buf: &mut [u8]) -> Result<(), Error<T::Error>> {
    //         let (request_buf, response_buf) = buf.split_at_mut(buf.len() / 2);

    //         let (raw_request, body) = super::receive(request_buf, &mut self.connection).await.map_err(|(_, e)| e)?;

    //         let method = crate::asynch::http::Method::new(raw_request.method.unwrap_or("GET"));

    //         let state = ServerRequestResponseState::Request(Some(ServerRequestState {
    //             request: raw_request,
    //             response_headers: todo!(),
    //             io: body,
    //         }));

    //         let request = ServerHandlerRequest(&mut state);

    //         if let Some(method) = method {
    //             let result = self
    //                 .registration
    //                 .handle(
    //                     false,
    //                     state.request().request.path.unwrap_or(""),
    //                     method.into(),
    //                     request,
    //                 )
    //                 .await;

    //             match result {
    //                 Result::Ok(_) => Ok(()),
    //                 Result::Err(e) => {
    //                     match state {
    //                         ServerRequestResponseState::Request(request) => todo!(),
    //                         ServerRequestResponseState::Response(request) => todo!(),
    //                         ServerRequestResponseState::Response(request) => todo!(),
    //                         ServerRequestResponseState::ResponseWrite(request) => todo!(),

    //                     }
    //                     let (read_state, write_state) = self.connection.completion();

    //                     if write_state == CompletionState::NotStarted
    //                         && read_state == CompletionState::NotStarted
    //                     {
    //                         let request = ServerRequest::wrap(
    //                             &raw_request,
    //                             body_buf,
    //                             read_len,
    //                             response_buf,
    //                             &mut self.connection,
    //                         );

    //                         request
    //                             .into_response()
    //                             .await?
    //                             .status(500)
    //                             .status_message(e.message())
    //                             .complete()
    //                             .await?;

    //                         Ok(())
    //                     } else {
    //                         Err(Error::IncompleteBody)
    //                     }
    //                 }
    //             }
    //         } else {
    //             ().handle(true, raw_request.path.unwrap_or(""), Method::Get, request)
    //                 .await
    //                 .map_err(|_| Error::InvalidBody)?;

    //             Ok(())
    //         }
    //     }
    // }
}
