#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::{mem, str};

    use no_std_net::SocketAddr;

    use embedded_svc::http::client::asynch::{Connection, Method};
    use embedded_svc::io::asynch::{Io, Read, Write};

    use crate::asynch::http::{
        send_headers, send_headers_end, send_request, Body, BodyType, Error,
        Response as RawResponse, SendBody,
    };
    use crate::asynch::tcp::TcpClientSocket;

    const COMPLETION_BUF_SIZE: usize = 64;

    pub enum ClientConnection<'b, const N: usize, T> {
        Transition(Transition),
        Unbound(Unbound<'b, N, T>),
        Request(Request<'b, N, T>),
        Response(Response<'b, N, T>),
    }

    impl<'b, const N: usize, T> ClientConnection<'b, N, T>
    where
        T: TcpClientSocket,
    {
        pub fn new(buf: &'b mut [u8], io: T, addr: SocketAddr) -> Self {
            Self::Unbound(Unbound { buf, io, addr })
        }

        async fn start_request<'a>(
            &'a mut self,
            method: Method,
            uri: &'a str,
            headers: &'a [(&'a str, &'a str)],
        ) -> Result<(), Error<T::Error>> {
            let _ = self.complete().await;

            let state = self.unbound()?;

            if !state.io.is_connected().await.map_err(Error::Io)? {
                // TODO: Need to validate that the socket is still alive

                state.io.connect(state.addr).await.map_err(Error::Io)?;
            }

            let mut state = self.unbind();

            let result = async {
                send_request(Some(method.into()), Some(uri), &mut state.io).await?;

                let body_type = send_headers(headers, &mut state.io).await?;
                send_headers_end(&mut state.io).await?;

                Ok(body_type)
            }
            .await;

            match result {
                Ok(body_type) => {
                    *self = Self::Request(Request {
                        buf: state.buf,
                        addr: state.addr,
                        io: SendBody::new(body_type, state.io),
                    });

                    Ok(())
                }
                Err(e) => {
                    state.io.close();
                    *self = Self::Unbound(state);

                    Err(e)
                }
            }
        }

        async fn complete(&mut self) -> Result<(), Error<T::Error>> {
            let result = async {
                if self.request().is_ok() {
                    self.complete_request().await?;
                }

                if self.response().is_ok() {
                    self.complete_response().await?;
                }

                Result::<(), Error<T::Error>>::Ok(())
            }
            .await;

            let mut state = self.unbind();

            if result.is_err() {
                state.io.close();
            }

            *self = Self::Unbound(state);

            result
        }

        async fn complete_request(&mut self) -> Result<(), Error<T::Error>>
        where
            T: Read + Write,
        {
            self.request()?.io.finish().await?;

            let mut state = self.unbind();
            let buf_ptr: *mut [u8] = state.buf;

            let mut response = RawResponse::new();

            match response.receive(state.buf, &mut state.io).await {
                Ok((buf, read_len)) => {
                    let io = Body::new(
                        BodyType::from_headers(response.headers.iter()),
                        buf,
                        read_len,
                        state.io,
                    );

                    *self = Self::Response(Response {
                        buf: buf_ptr,
                        addr: state.addr,
                        response,
                        io,
                    });

                    Ok(())
                }
                Err(e) => {
                    state.io.close();

                    state.buf = unsafe { buf_ptr.as_mut().unwrap() };

                    *self = Self::Unbound(state);

                    Err(e)
                }
            }
        }

        async fn complete_response<'a>(&mut self) -> Result<(), Error<T::Error>>
        where
            T: Read + Write,
        {
            if self.request().is_ok() {
                self.complete_request().await?;
            }

            let response = self.response()?;

            let mut buf = [0; COMPLETION_BUF_SIZE];
            while response.io.read(&mut buf).await? > 0 {}

            *self = Self::Unbound(self.unbind());

            Ok(())
        }

        fn unbind(&mut self) -> Unbound<'b, N, T> {
            let state = mem::replace(self, Self::Transition(Transition(())));

            let unbound = match state {
                Self::Unbound(unbound) => unbound,
                Self::Request(request) => {
                    let io = request.io.release();

                    Unbound {
                        buf: request.buf,
                        addr: request.addr,
                        io,
                    }
                }
                Self::Response(response) => {
                    let io = response.io.release();

                    Unbound {
                        buf: unsafe { response.buf.as_mut().unwrap() },
                        addr: response.addr,
                        io,
                    }
                }
                _ => unreachable!(),
            };

            unbound
        }

        fn unbound(&mut self) -> Result<&mut Unbound<'b, N, T>, Error<T::Error>>
        where
            T: Io,
        {
            if let Self::Unbound(new) = self {
                Ok(new)
            } else {
                Err(Error::InvalidState)
            }
        }

        fn request(&mut self) -> Result<&mut Request<'b, N, T>, Error<T::Error>>
        where
            T: Io,
        {
            if let Self::Request(request) = self {
                Ok(request)
            } else {
                Err(Error::InvalidState)
            }
        }

        fn response(&mut self) -> Result<&mut Response<'b, N, T>, Error<T::Error>>
        where
            T: Io,
        {
            if let Self::Response(response) = self {
                Ok(response)
            } else {
                Err(Error::InvalidState)
            }
        }

        fn response_ref(&self) -> Result<&Response<'b, N, T>, Error<T::Error>>
        where
            T: Io,
        {
            if let Self::Response(response) = self {
                Ok(response)
            } else {
                Err(Error::InvalidState)
            }
        }

        fn raw_io(&mut self) -> &mut T
        where
            T: Read + Write,
        {
            match self {
                Self::Unbound(unbound) => &mut unbound.io,
                Self::Request(request) => request.io.as_raw_writer(),
                Self::Response(response) => response.io.as_raw_reader(),
                _ => unreachable!(),
            }
        }
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
            async move { self.start_request(method, uri, headers).await }
        }

        fn request(&mut self) -> Result<&mut Self::Write, Self::Error> {
            Ok(&mut self.request()?.io)
        }

        fn initiate_response(&mut self) -> Self::IntoResponseFuture<'_> {
            async move { self.complete_request().await }
        }

        fn response(&mut self) -> Result<(&Self::Headers, &mut Self::Read), Self::Error> {
            let response = self.response()?;

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

    pub struct Transition(());

    pub struct Unbound<'b, const N: usize, T> {
        buf: &'b mut [u8],
        io: T,
        addr: SocketAddr,
    }

    pub struct Request<'b, const N: usize, T> {
        buf: &'b mut [u8],
        io: SendBody<T>,
        addr: SocketAddr,
    }

    pub struct Response<'b, const N: usize, T> {
        buf: *mut [u8],
        response: RawResponse<'b, N>,
        io: Body<'b, T>,
        addr: SocketAddr,
    }
}
