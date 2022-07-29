#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::str;

    use no_std_net::SocketAddr;

    use embedded_svc::http::client::asynch::{Connection, Method};
    use embedded_svc::io::asynch::{Io, Read, Write};

    use crate::asynch::http::{
        send_headers, send_headers_end, send_request, Body, BodyType, Error, Response, SendBody,
    };
    use crate::asynch::tcp::TcpClientSocket;

    pub enum ClientConnection<'b, const N: usize, T> {
        NewState(Option<ClientNewState<'b, N, T>>),
        RequestState(Option<ClientRequestState<'b, N, T>>),
        ResponseState(Option<ClientResponseState<'b, N, T>>),
    }

    impl<'b, const N: usize, T> ClientConnection<'b, N, T>
    where
        T: TcpClientSocket,
    {
        pub fn new(buf: &'b mut [u8], socket: T, addr: SocketAddr) -> Self {
            Self::NewState(Some(ClientNewState { buf, socket, addr }))
        }

        fn new_state(&mut self) -> Result<&mut ClientNewState<'b, N, T>, Error<T::Error>>
        where
            T: Io,
        {
            match self {
                Self::NewState(new) => Ok(new.as_mut().unwrap()),
                _ => Err(Error::InvalidState),
            }
        }

        fn request_write(&mut self) -> Result<&mut SendBody<T>, Error<T::Error>>
        where
            T: Io,
        {
            match self {
                Self::RequestState(request) => Ok(&mut request.as_mut().unwrap().io),
                _ => Err(Error::InvalidState),
            }
        }

        fn response_mut(&mut self) -> Result<&mut ClientResponseState<'b, N, T>, Error<T::Error>>
        where
            T: Io,
        {
            match self {
                Self::ResponseState(response) => Ok(response.as_mut().unwrap()),
                _ => Err(Error::InvalidState),
            }
        }

        fn response_ref(&self) -> Result<&ClientResponseState<'b, N, T>, Error<T::Error>>
        where
            T: Io,
        {
            match self {
                Self::ResponseState(response) => Ok(response.as_ref().unwrap()),
                _ => Err(Error::InvalidState),
            }
        }

        fn raw_io(&mut self) -> &mut T
        where
            T: Read + Write,
        {
            match self {
                Self::NewState(new) => &mut new.as_mut().unwrap().socket,
                Self::RequestState(request) => request.as_mut().unwrap().io.as_raw_writer(),
                Self::ResponseState(response) => response.as_mut().unwrap().io.as_raw_reader(),
            }
        }

        // TODO: Completion
    }

    impl<'b, const N: usize, T> Io for ClientConnection<'b, N, T>
    where
        T: Io,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, const N: usize, T> Connection for ClientConnection<'b, N, T>
    where
        T: TcpClientSocket,
    {
        type Read = Body<'b, T>;

        type Write = SendBody<T>;

        type Headers = crate::asynch::http::Response<'b, N>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        type IntoRequestFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        type IntoResponseFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        fn initiate_request<'a>(
            &'a mut self,
            method: Method,
            uri: &'a str,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::IntoRequestFuture<'a> {
            async move {
                match self {
                    Self::NewState(new) => {
                        let mut new = new.take().unwrap();

                        if !new.socket.is_connected().await.map_err(Error::Io)? {
                            // TODO: Need to validate that the socket is still alive

                            new.socket.connect(new.addr).await.map_err(Error::Io)?;
                        }

                        send_request(Some(method.into()), Some(uri), &mut new.socket).await?;
                        let body_type = send_headers(headers, &mut new.socket).await?;
                        send_headers_end(&mut new.socket).await?;

                        *self = Self::RequestState(Some(ClientRequestState {
                            buf: new.buf,
                            addr: new.addr,
                            io: SendBody::new(body_type, new.socket),
                        }));

                        Ok(())
                    }
                    _ => Err(Error::InvalidState),
                }
            }
        }

        fn request(&mut self) -> Result<&mut Self::Write, Self::Error> {
            self.request_write()
        }

        fn initiate_response(&mut self) -> Self::IntoResponseFuture<'_> {
            async move {
                match self {
                    Self::RequestState(request) => {
                        let request = request.take().unwrap();

                        let buf = request.buf;
                        let mut io = request.io.release();

                        let mut raw_response = Response::new();

                        let (buf, read_len) = raw_response.receive(buf, &mut io).await?;

                        let buf_ptr: *mut [u8] = buf; // TODO

                        let body = Body::new(
                            BodyType::from_headers(raw_response.headers.iter()),
                            buf,
                            read_len,
                            io,
                        );

                        *self = Self::ResponseState(Some(ClientResponseState {
                            buf: buf_ptr,
                            addr: request.addr,
                            response: raw_response,
                            io: body,
                        }));

                        Ok(())
                    }
                    _ => Err(Error::InvalidState),
                }
            }
        }

        fn response(&mut self) -> Result<(&Self::Headers, &mut Self::Read), Self::Error> {
            let response = self.response_mut()?;

            Ok((&response.response, &mut response.io))
        }

        fn headers(&self) -> Result<&Self::Headers, Self::Error> {
            let response = self.response_ref()?;

            Ok(&response.response)
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            Ok(self.raw_io())
        }
    }

    pub struct ClientNewState<'b, const N: usize, T> {
        buf: &'b mut [u8],
        socket: T,
        addr: SocketAddr,
    }

    pub struct ClientRequestState<'b, const N: usize, T> {
        buf: &'b mut [u8],
        io: SendBody<T>,
        addr: SocketAddr,
    }

    pub struct ClientResponseState<'b, const N: usize, T> {
        buf: *mut [u8],
        response: Response<'b, N>,
        io: Body<'b, T>,
        addr: SocketAddr,
    }
}
