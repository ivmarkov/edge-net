#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::str;

    use no_std_net::SocketAddr;

    use embedded_svc::http::client::asynch::Method;
    use embedded_svc::io::asynch::{Io, Read, Write};

    use crate::asynch::http::completion::{
        BodyCompletionTracker, Complete, CompletionTracker, SendBodyCompletionTracker,
    };
    use crate::asynch::http::{
        send_headers, send_headers_end, send_request, BodyType, Error, PartiallyRead, Response,
    };
    use crate::asynch::tcp::TcpClientSocket;
    use crate::close::Close;

    pub struct Client<'b, const N: usize, T>
    where
        T: TcpClientSocket + 'b,
    {
        buf: &'b mut [u8],
        socket: T,
        addr: SocketAddr,
    }

    impl<'b, const N: usize, T> Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        pub fn new(buf: &'b mut [u8], socket: T, addr: SocketAddr) -> Self {
            Self { buf, socket, addr }
        }
    }

    impl<'b, const N: usize, T> embedded_svc::io::asynch::Io for Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Client for Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        type RequestWrite<'a>
        where
            Self: 'a,
        = ClientRequestWrite<'a, N, CompletionTracker<&'a mut T>>;

        type RequestFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<Self::RequestWrite<'a>, Self::Error>>;

        fn request<'a, H>(
            &'a mut self,
            method: Method,
            uri: &'a str,
            headers: H,
        ) -> Self::RequestFuture<'a>
        where
            H: IntoIterator<Item = (&'a str, &'a str)>,
        {
            async move {
                if !self.socket.is_connected().await.map_err(Error::Io)? {
                    // TODO: Need to validate that the socket is still alive

                    self.socket.connect(self.addr).await.map_err(Error::Io)?;
                }

                Ok(Self::RequestWrite::new(
                    method,
                    uri,
                    headers,
                    self.buf,
                    CompletionTracker::new(&mut self.socket),
                ))
            }
        }
    }

    pub struct ClientRequestWrite<'b, const N: usize, T> {
        buf: &'b mut [u8],
        io: SendBodyCompletionTracker<T>,
    }

    impl<'b, const N: usize, T> ClientRequestWrite<'b, N, T>
    where
        T: Write,
    {
        async fn new<'a, H>(
            method: Method,
            uri: &'a str,
            headers: H,
            buf: &'b mut [u8],
            mut io: T,
        ) -> Result<ClientRequestWrite<'b, N, T>, Error<T::Error>>
        where
            H: IntoIterator<Item = (&'a str, &'a str)>,
        {
            send_request(Some(method.into()), Some(uri), &mut io).await?;
            let body_type = send_headers(headers, &mut io).await?;
            send_headers_end(&mut io).await?;

            Ok(Self {
                buf,
                io: SendBodyCompletionTracker::new(body_type, io),
            })
        }
    }

    impl<'b, const N: usize, T> Io for ClientRequestWrite<'b, N, T>
    where
        T: Io,
    {
        type Error = T::Error;
    }

    impl<'b, const N: usize, T> Write for ClientRequestWrite<'b, N, T>
    where
        T: Write + Close + Complete,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = T::WriteFuture<'a>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            self.io.write(buf)
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = T::FlushFuture<'a>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            self.io.flush()
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::RequestWrite
        for ClientRequestWrite<'b, N, T>
    where
        T: Read + Write + Close + Complete,
    {
        type Response = ClientResponse<'b, N, BodyCompletionTracker<'b, T>>;

        type IntoResponseFuture = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn submit(mut self) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                self.io.flush().await?;

                if !self.io.is_complete() {
                    self.io.close();

                    Err(Error::IncompleteBody)
                } else {
                    let mut response = crate::asynch::http::Response::new();
                    let io = self.io.release().release();

                    match response.receive(self.buf, &mut io).await {
                        Ok((buf, read_len)) => Ok(Self::Response {
                            response,
                            io: BodyCompletionTracker::new(
                                BodyType::from_headers(&response.headers),
                                buf,
                                read_len,
                                io,
                            ),
                        }),
                        Err((mut io, e)) => {
                            io.close();

                            Err(e)
                        }
                    }
                }
            }
        }
    }

    pub struct ClientResponse<'b, const N: usize, T> {
        response: Response<'b, N>,
        io: BodyCompletionTracker<'b, PartiallyRead<'b, T>>,
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Status
        for ClientResponse<'b, N, T>
    {
        fn status(&self) -> u16 {
            self.response.code.unwrap_or(200)
        }

        fn status_message(&self) -> Option<&'_ str> {
            self.response.reason
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Headers
        for ClientResponse<'b, N, T>
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.response.headers.header(name)
        }
    }

    impl<'b, const N: usize, R> embedded_svc::io::Io for ClientResponse<'b, N, R>
    where
        R: Io,
    {
        type Error = R::Error;
    }

    impl<'b, const N: usize, R> embedded_svc::io::asynch::Read for ClientResponse<'b, N, R>
    where
        R: Read + Close,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = R::ReadFuture<'a>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            self.io.read(buf)
        }
    }

    impl<'b, const N: usize, R> embedded_svc::http::client::asynch::Response
        for ClientResponse<'b, N, R>
    where
        R: Read + Close,
    {
        type Headers<'a> = &'a Response<'a, N>;

        type Read<'a> = &'a mut BodyCompletionTracker<R>;

        fn split<'a>(&mut self) -> (Self::Headers<'a>, Self::Read<'a>)
        where
            Self: Sized,
        {
            (&self.response, &mut self.io)
        }
    }
}
