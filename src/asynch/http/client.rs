#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::fmt::Display;
    use core::future::Future;
    use core::str;

    use no_std_net::SocketAddr;

    use embedded_svc::http::client::asynch::Method;
    use embedded_svc::io::asynch::{Io, Read, Write};

    use crate::asynch::http::completion::{
        BodyCompletionTracker, CompletionTracker, SendBodyCompletionTracker,
    };
    use crate::asynch::http::{
        send_headers, send_headers_end, send_request, BodyType, Error, Response,
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
        = ClientRequestWrite<'a, N, &'a mut T>;

        type RequestFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<Self::RequestWrite<'a>, Self::Error>>;

        fn request<'a>(
            &'a mut self,
            method: Method,
            uri: &'a str,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::RequestFuture<'a> {
            async move {
                if !self.socket.is_connected().await.map_err(Error::Io)? {
                    // TODO: Need to validate that the socket is still alive

                    self.socket.connect(self.addr).await.map_err(Error::Io)?;
                }

                let write = Self::RequestWrite::new(
                    method,
                    uri,
                    headers,
                    self.buf,
                    CompletionTracker::new(&mut self.socket),
                )
                .await?;

                Ok(write)
            }
        }
    }

    pub struct ClientRequestWrite<'b, const N: usize, T>
    where
        T: Close,
    {
        buf: &'b mut [u8],
        io: SendBodyCompletionTracker<T>,
    }

    impl<'b, const N: usize, T> ClientRequestWrite<'b, N, T>
    where
        T: Write + Close,
    {
        pub async fn new<'a>(
            method: Method,
            uri: &'a str,
            headers: &'a [(&'a str, &'a str)],
            buf: &'b mut [u8],
            mut io: CompletionTracker<T>,
        ) -> Result<ClientRequestWrite<'b, N, T>, Error<T::Error>> {
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
        T: Io + Close,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, const N: usize, T> Write for ClientRequestWrite<'b, N, T>
    where
        T: Write + Close,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            async move { self.io.write(buf).await }
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            async move { self.io.flush().await }
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::RequestWrite
        for ClientRequestWrite<'b, N, T>
    where
        T: Read + Write + Close,
    {
        type Response = ClientResponse<'b, N, T>;

        type IntoResponseFuture = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn submit(mut self) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                self.io.flush().await?;

                if !self.io.body().is_complete() {
                    self.io.body().close();

                    Err(Error::IncompleteBody)
                } else {
                    let mut response = crate::asynch::http::Response::new();
                    let mut io = self.io.release().release();

                    match response.receive(self.buf, &mut io).await {
                        Ok((buf, read_len)) => {
                            let body_type = BodyType::from_headers(response.headers.iter());

                            Ok(Self::Response {
                                response,
                                io: BodyCompletionTracker::new(body_type, buf, read_len, io),
                            })
                        }
                        Err(e) => {
                            io.close();

                            Err(e)
                        }
                    }
                }
            }
        }
    }

    pub struct ClientResponse<'b, const N: usize, T>
    where
        T: Close,
    {
        response: Response<'b, N>,
        io: BodyCompletionTracker<'b, T>,
    }

    impl<'b, const N: usize, T> Display for ClientResponse<'b, N, T>
    where
        T: Close,
    {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            self.response.fmt(f)
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Status for ClientResponse<'b, N, T>
    where
        T: Close,
    {
        fn status(&self) -> u16 {
            self.response.code.unwrap_or(200)
        }

        fn status_message(&self) -> Option<&'_ str> {
            self.response.reason
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Headers for ClientResponse<'b, N, T>
    where
        T: Close,
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.response.headers.header(name)
        }
    }

    impl<'b, const N: usize, T> embedded_svc::io::Io for ClientResponse<'b, N, T>
    where
        T: Io + Close,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, const N: usize, T> embedded_svc::io::asynch::Read for ClientResponse<'b, N, T>
    where
        T: Read + Close,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move { self.io.read(buf).await }
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Response
        for ClientResponse<'b, N, T>
    where
        T: Read + Close,
    {
        type Headers = Response<'b, N>;

        type Read = BodyCompletionTracker<'b, T>;

        fn split<'a>(&'a mut self) -> (&'a Self::Headers, &'a mut Self::Read)
        where
            Self: Sized,
        {
            (&self.response, &mut self.io)
        }
    }
}
