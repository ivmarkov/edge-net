use core::mem;
use core::net::SocketAddr;
use core::str;

use embedded_io_async::{ErrorType, Read, Write};

use edge_nal::TcpConnect;

use crate::{
    ws::{upgrade_request_headers, MAX_BASE64_KEY_LEN, MAX_BASE64_KEY_RESPONSE_LEN, NONCE_LEN},
    DEFAULT_MAX_HEADERS_COUNT,
};

use super::{
    send_headers, send_headers_end, send_request, Body, BodyType, Error, ResponseHeaders, SendBody,
};

#[allow(unused_imports)]
#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

use super::Method;

const COMPLETION_BUF_SIZE: usize = 64;

#[allow(private_interfaces)]
pub enum Connection<'b, T, const N: usize = DEFAULT_MAX_HEADERS_COUNT>
where
    T: TcpConnect,
{
    Transition(TransitionState),
    Unbound(UnboundState<'b, T, N>),
    Request(RequestState<'b, T, N>),
    Response(ResponseState<'b, T, N>),
}

impl<'b, T, const N: usize> Connection<'b, T, N>
where
    T: TcpConnect,
{
    pub fn new(buf: &'b mut [u8], socket: &'b T, addr: SocketAddr) -> Self {
        Self::Unbound(UnboundState {
            buf,
            socket,
            addr,
            io: None,
        })
    }

    pub async fn reinitialize(&mut self, addr: SocketAddr) -> Result<(), Error<T::Error>> {
        let _ = self.complete().await;
        self.unbound_mut().unwrap().addr = addr;

        Ok(())
    }

    pub async fn initiate_request(
        &mut self,
        http11: bool,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> Result<(), Error<T::Error>> {
        self.start_request(http11, method, uri, headers).await
    }

    pub fn is_request_initiated(&self) -> bool {
        matches!(self, Self::Request(_))
    }

    pub async fn initiate_response(&mut self) -> Result<(), Error<T::Error>> {
        self.complete_request().await
    }

    pub fn is_response_initiated(&self) -> bool {
        matches!(self, Self::Response(_))
    }

    pub async fn initiate_ws_upgrade_request(
        &mut self,
        host: Option<&str>,
        origin: Option<&str>,
        uri: &str,
        version: Option<&str>,
        nonce: &[u8; NONCE_LEN],
        nonce_base64_buf: &mut [u8; MAX_BASE64_KEY_LEN],
    ) -> Result<(), Error<T::Error>> {
        let headers = upgrade_request_headers(host, origin, version, nonce, nonce_base64_buf);

        self.initiate_request(true, Method::Get, uri, &headers)
            .await
    }

    pub fn is_ws_upgrade_accepted(
        &self,
        nonce: &[u8; NONCE_LEN],
        buf: &mut [u8; MAX_BASE64_KEY_RESPONSE_LEN],
    ) -> Result<bool, Error<T::Error>> {
        Ok(self.headers()?.is_ws_upgrade_accepted(nonce, buf))
    }

    #[allow(clippy::type_complexity)]
    pub fn split(&mut self) -> (&ResponseHeaders<'b, N>, &mut Body<'b, T::Socket<'b>>) {
        let response = self.response_mut().expect("Not in response mode");

        (&response.response, &mut response.io)
    }

    pub fn headers(&self) -> Result<&ResponseHeaders<'b, N>, Error<T::Error>> {
        let response = self.response_ref()?;

        Ok(&response.response)
    }

    pub fn raw_connection(&mut self) -> Result<&mut T::Socket<'b>, Error<T::Error>> {
        Ok(self.io_mut())
    }

    pub fn release(mut self) -> (T::Socket<'b>, &'b mut [u8]) {
        let mut state = self.unbind();

        let io = state.io.take().unwrap();

        (io, state.buf)
    }

    async fn start_request(
        &mut self,
        http11: bool,
        method: Method,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> Result<(), Error<T::Error>> {
        let _ = self.complete().await;

        let state = self.unbound_mut()?;

        let fresh_connection = if state.io.is_none() {
            state.io = Some(state.socket.connect(state.addr).await.map_err(Error::Io)?.1);
            true
        } else {
            false
        };

        let mut state = self.unbind();

        let result = async {
            match send_request(http11, Some(method), Some(uri), state.io.as_mut().unwrap()).await {
                Ok(_) => (),
                Err(Error::Io(_)) => {
                    if !fresh_connection {
                        // Attempt to reconnect and re-send the request
                        state.io = None;
                        state.io =
                            Some(state.socket.connect(state.addr).await.map_err(Error::Io)?.1);

                        send_request(http11, Some(method), Some(uri), state.io.as_mut().unwrap())
                            .await?;
                    }
                }
                Err(other) => Err(other)?,
            }

            let io = state.io.as_mut().unwrap();

            let body_type = send_headers(headers, &mut *io).await?;
            send_headers_end(io).await?;

            Ok(body_type)
        }
        .await;

        match result {
            Ok(body_type) => {
                *self = Self::Request(RequestState {
                    buf: state.buf,
                    socket: state.socket,
                    addr: state.addr,
                    io: SendBody::new(body_type, state.io.unwrap()),
                });

                Ok(())
            }
            Err(e) => {
                state.io = None;
                *self = Self::Unbound(state);

                Err(e)
            }
        }
    }

    pub async fn complete(&mut self) -> Result<(), Error<T::Error>> {
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
            state.io = None;
        }

        *self = Self::Unbound(state);

        result
    }

    async fn complete_request(&mut self) -> Result<(), Error<T::Error>> {
        self.request_mut()?.io.finish().await?;

        let mut state = self.unbind();
        let buf_ptr: *mut [u8] = state.buf;

        let mut response = ResponseHeaders::new();

        match response
            .receive(state.buf, &mut state.io.as_mut().unwrap())
            .await
        {
            Ok((buf, read_len)) => {
                let io = Body::new(
                    BodyType::from_headers(response.headers.iter()),
                    buf,
                    read_len,
                    state.io.unwrap(),
                );

                *self = Self::Response(ResponseState {
                    buf: buf_ptr,
                    response,
                    socket: state.socket,
                    addr: state.addr,
                    io,
                });

                Ok(())
            }
            Err(e) => {
                state.io = None;
                state.buf = unsafe { buf_ptr.as_mut().unwrap() };

                *self = Self::Unbound(state);

                Err(e)
            }
        }
    }

    async fn complete_response(&mut self) -> Result<(), Error<T::Error>> {
        if self.request_mut().is_ok() {
            self.complete_request().await?;
        }

        let response = self.response_mut()?;

        let mut buf = [0; COMPLETION_BUF_SIZE];
        while response.io.read(&mut buf).await? > 0 {}

        *self = Self::Unbound(self.unbind());

        Ok(())
    }

    fn unbind(&mut self) -> UnboundState<'b, T, N> {
        let state = mem::replace(self, Self::Transition(TransitionState(())));

        let unbound = match state {
            Self::Unbound(unbound) => unbound,
            Self::Request(request) => {
                let io = request.io.release();

                UnboundState {
                    buf: request.buf,
                    socket: request.socket,
                    addr: request.addr,
                    io: Some(io),
                }
            }
            Self::Response(response) => {
                let io = response.io.release();

                UnboundState {
                    buf: unsafe { response.buf.as_mut().unwrap() },
                    socket: response.socket,
                    addr: response.addr,
                    io: Some(io),
                }
            }
            _ => unreachable!(),
        };

        unbound
    }

    fn unbound_mut(&mut self) -> Result<&mut UnboundState<'b, T, N>, Error<T::Error>> {
        if let Self::Unbound(new) = self {
            Ok(new)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn request_mut(&mut self) -> Result<&mut RequestState<'b, T, N>, Error<T::Error>> {
        if let Self::Request(request) = self {
            Ok(request)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn response_mut(&mut self) -> Result<&mut ResponseState<'b, T, N>, Error<T::Error>> {
        if let Self::Response(response) = self {
            Ok(response)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn response_ref(&self) -> Result<&ResponseState<'b, T, N>, Error<T::Error>> {
        if let Self::Response(response) = self {
            Ok(response)
        } else {
            Err(Error::InvalidState)
        }
    }

    fn io_mut(&mut self) -> &mut T::Socket<'b> {
        match self {
            Self::Unbound(unbound) => unbound.io.as_mut().unwrap(),
            Self::Request(request) => request.io.as_raw_writer(),
            Self::Response(response) => response.io.as_raw_reader(),
            _ => unreachable!(),
        }
    }
}

impl<'b, T, const N: usize> ErrorType for Connection<'b, T, N>
where
    T: TcpConnect,
{
    type Error = Error<T::Error>;
}

impl<'b, T, const N: usize> Read for Connection<'b, T, N>
where
    T: TcpConnect,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.response_mut()?.io.read(buf).await
    }
}

impl<'b, T, const N: usize> Write for Connection<'b, T, N>
where
    T: TcpConnect,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.request_mut()?.io.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.request_mut()?.io.flush().await
    }
}

struct TransitionState(());

struct UnboundState<'b, T, const N: usize>
where
    T: TcpConnect,
{
    buf: &'b mut [u8],
    socket: &'b T,
    addr: SocketAddr,
    io: Option<T::Socket<'b>>,
}

struct RequestState<'b, T, const N: usize>
where
    T: TcpConnect,
{
    buf: &'b mut [u8],
    socket: &'b T,
    addr: SocketAddr,
    io: SendBody<T::Socket<'b>>,
}

struct ResponseState<'b, T, const N: usize>
where
    T: TcpConnect,
{
    buf: *mut [u8],
    response: ResponseHeaders<'b, N>,
    socket: &'b T,
    addr: SocketAddr,
    io: Body<'b, T::Socket<'b>>,
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use super::*;

    use embedded_svc::http::client::asynch::{Connection, Headers, Method, Status};

    impl<'b, T, const N: usize> Headers for super::Connection<'b, T, N>
    where
        T: TcpConnect,
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            let response = self.response_ref().expect("Not in response state");

            response.response.header(name)
        }
    }

    impl<'b, T, const N: usize> Status for super::Connection<'b, T, N>
    where
        T: TcpConnect,
    {
        fn status(&self) -> u16 {
            let response = self.response_ref().expect("Not in response state");

            response.response.status()
        }

        fn status_message(&self) -> Option<&'_ str> {
            let response = self.response_ref().expect("Not in response state");

            response.response.status_message()
        }
    }

    impl<'b, T, const N: usize> Connection for super::Connection<'b, T, N>
    where
        T: TcpConnect,
    {
        type Read = Body<'b, T::Socket<'b>>;

        type Headers = ResponseHeaders<'b, N>;

        type RawConnectionError = T::Error;

        type RawConnection = T::Socket<'b>;

        async fn initiate_request(
            &mut self,
            method: Method,
            uri: &str,
            headers: &[(&str, &str)],
        ) -> Result<(), Self::Error> {
            super::Connection::initiate_request(self, true, method.into(), uri, headers).await
        }

        fn is_request_initiated(&self) -> bool {
            super::Connection::is_request_initiated(self)
        }

        async fn initiate_response(&mut self) -> Result<(), Self::Error> {
            super::Connection::initiate_response(self).await
        }

        fn is_response_initiated(&self) -> bool {
            super::Connection::is_response_initiated(self)
        }

        fn split(&mut self) -> (&Self::Headers, &mut Self::Read) {
            super::Connection::split(self)
        }

        fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
            // TODO: Needs a GAT rather than `&mut` return type
            // or `embedded-svc` fully upgraded to async traits & `embedded-io` 0.4 to re-enable
            //ClientConnection::raw_connection(self).map(EmbIo)
            panic!("Not supported")
        }
    }
}
