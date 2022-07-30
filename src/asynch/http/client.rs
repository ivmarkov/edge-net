use core::future::Future;
use core::{mem, str};

use embedded_io::asynch::Read;
use no_std_net::SocketAddr;

use crate::asynch::http::{
    send_headers, send_headers_end, send_request, Body, BodyType, Error, ResponseHeaders, SendBody,
};
use crate::asynch::tcp::TcpClientSocket;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

use super::Method;

const COMPLETION_BUF_SIZE: usize = 64;

pub enum ClientConnection<'b, const N: usize, T> {
    Transition(TransitionState),
    Unbound(UnboundState<'b, N, T>),
    Request(RequestState<'b, N, T>),
    Response(ResponseState<'b, N, T>),
}

impl<'b, const N: usize, T> ClientConnection<'b, N, T>
where
    T: TcpClientSocket,
{
    pub fn new(buf: &'b mut [u8], io: T, addr: SocketAddr) -> Self {
        Self::Unbound(UnboundState { buf, io, addr })
    }

    pub async fn initiate_request<'a>(
        &'a mut self,
        method: Method,
        uri: &'a str,
        headers: &'a [(&'a str, &'a str)],
    ) -> Result<(), Error<T::Error>> {
        self.start_request(method, uri, headers).await
    }

    pub fn request(&mut self) -> Result<&mut SendBody<T>, Error<T::Error>> {
        Ok(&mut self.request_mut()?.io)
    }

    pub async fn initiate_response(&mut self) -> Result<(), Error<T::Error>> {
        self.complete_request().await
    }

    pub fn response(
        &mut self,
    ) -> Result<(&ResponseHeaders<'b, N>, &mut Body<'b, T>), Error<T::Error>> {
        let response = self.response_mut()?;

        Ok((&response.response, &mut response.io))
    }

    pub fn headers(&self) -> Result<&ResponseHeaders<'b, N>, Error<T::Error>> {
        let response = self.response_ref()?;

        Ok(&response.response)
    }

    pub fn raw_connection(&mut self) -> Result<&mut T, Error<T::Error>> {
        Ok(self.io_mut())
    }

    async fn start_request<'a>(
        &'a mut self,
        method: Method,
        uri: &'a str,
        headers: &'a [(&'a str, &'a str)],
    ) -> Result<(), Error<T::Error>> {
        let _ = self.complete().await;

        let state = self.unbound_mut()?;

        if !state.io.is_connected().await.map_err(Error::Io)? {
            // TODO: Need to validate that the socket is still alive

            state.io.connect(state.addr).await.map_err(Error::Io)?;
        }

        let mut state = self.unbind();

        let result = async {
            send_request(Some(method), Some(uri), &mut state.io).await?;

            let body_type = send_headers(headers, &mut state.io).await?;
            send_headers_end(&mut state.io).await?;

            Ok(body_type)
        }
        .await;

        match result {
            Ok(body_type) => {
                *self = Self::Request(RequestState {
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
            if self.request_mut().is_ok() {
                self.complete_request().await?;
            }

            if self.response_mut().is_ok() {
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

    async fn complete_request(&mut self) -> Result<(), Error<T::Error>> {
        self.request_mut()?.io.finish().await?;

        let mut state = self.unbind();
        let buf_ptr: *mut [u8] = state.buf;

        let mut response = ResponseHeaders::new();

        match response.receive(state.buf, &mut state.io).await {
            Ok((buf, read_len)) => {
                let io = Body::new(
                    BodyType::from_headers(response.headers.iter()),
                    buf,
                    read_len,
                    state.io,
                );

                *self = Self::Response(ResponseState {
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

    async fn complete_response<'a>(&mut self) -> Result<(), Error<T::Error>> {
        if self.request_mut().is_ok() {
            self.complete_request().await?;
        }

        let response = self.response_mut()?;

        let mut buf = [0; COMPLETION_BUF_SIZE];
        while response.io.read(&mut buf).await? > 0 {}

        *self = Self::Unbound(self.unbind());

        Ok(())
    }

    fn unbind(&mut self) -> UnboundState<'b, N, T> {
        let state = mem::replace(self, Self::Transition(TransitionState(())));

        let unbound = match state {
            Self::Unbound(unbound) => unbound,
            Self::Request(request) => {
                let io = request.io.release();

                UnboundState {
                    buf: request.buf,
                    addr: request.addr,
                    io,
                }
            }
            Self::Response(response) => {
                let io = response.io.release();

                UnboundState {
                    buf: unsafe { response.buf.as_mut().unwrap() },
                    addr: response.addr,
                    io,
                }
            }
            _ => unreachable!(),
        };

        unbound
    }

    fn unbound_mut(&mut self) -> Result<&mut UnboundState<'b, N, T>, Error<T::Error>> {
        if let Self::Unbound(new) = self {
            Ok(new)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn request_mut(&mut self) -> Result<&mut RequestState<'b, N, T>, Error<T::Error>> {
        if let Self::Request(request) = self {
            Ok(request)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn response_mut(&mut self) -> Result<&mut ResponseState<'b, N, T>, Error<T::Error>> {
        if let Self::Response(response) = self {
            Ok(response)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn response_ref(&self) -> Result<&ResponseState<'b, N, T>, Error<T::Error>> {
        if let Self::Response(response) = self {
            Ok(response)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn io_mut(&mut self) -> &mut T {
        match self {
            Self::Unbound(unbound) => &mut unbound.io,
            Self::Request(request) => request.io.as_raw_writer(),
            Self::Response(response) => response.io.as_raw_reader(),
            _ => unreachable!(),
        }
    }
}

pub struct TransitionState(());

pub struct UnboundState<'b, const N: usize, T> {
    buf: &'b mut [u8],
    io: T,
    addr: SocketAddr,
}

pub struct RequestState<'b, const N: usize, T> {
    buf: &'b mut [u8],
    io: SendBody<T>,
    addr: SocketAddr,
}

pub struct ResponseState<'b, const N: usize, T> {
    buf: *mut [u8],
    response: ResponseHeaders<'b, N>,
    io: Body<'b, T>,
    addr: SocketAddr,
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use super::*;

    use embedded_svc::http::client::asynch::{Connection, Method};
    use embedded_svc::io::asynch::Io;

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

        type Headers = ResponseHeaders<'b, N>;

        type RawConnectionError = T::Error;

        type RawConnection = T;

        type IntoRequestFuture<'a>
        = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

        type IntoResponseFuture<'a>
        = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

        fn initiate_request<'a>(
            &'a mut self,
            method: Method,
            uri: &'a str,
            headers: &'a [(&'a str, &'a str)],
        ) -> Self::IntoRequestFuture<'a> {
            async move { ClientConnection::initiate_request(self, method.into(), uri, headers).await }
        }

        fn request(&mut self) -> Result<&mut Self::Write, Self::Error> {
            ClientConnection::request(self)
        }

        fn initiate_response(&mut self) -> Self::IntoResponseFuture<'_> {
            async move { ClientConnection::initiate_response(self).await }
        }

        fn response(&mut self) -> Result<(&Self::Headers, &mut Self::Read), Self::Error> {
            ClientConnection::response(self)
        }

        fn headers(&self) -> Result<&Self::Headers, Self::Error> {
            ClientConnection::headers(self)
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            ClientConnection::raw_connection(self)
        }
    }
}
