use core::str;

use embedded_io::asynch::Read;

use httparse::{Header, Status, EMPTY_HEADER};
use uncased::UncasedStr;

use crate::asynch::io;

use super::*;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

pub struct Request<'b>(SendHeaders<'b>);

impl<'b> Request<'b> {
    pub fn get(uri: &str, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Get, uri, buf)
    }

    pub fn post(uri: &str, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Post, uri, buf)
    }

    pub fn put(uri: &str, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Put, uri, buf)
    }

    pub fn delete(uri: &str, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Delete, uri, buf)
    }

    pub fn new(method: Method, uri: &str, buf: &'b mut [u8]) -> Self {
        let mut this = Self(SendHeaders::new(buf));

        this.0
            .set_status_tokens(&[method.as_str(), "HTTP/1.1", uri]);

        this
    }

    pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
        self.0.set(name, value);
        self
    }

    pub fn header_raw(&mut self, name: &str, value: &[u8]) -> &mut Self {
        self.0.set_raw(name, value);
        self
    }

    pub fn content_len(&mut self, content_len: usize) -> &mut Self {
        let content_len_str = heapless::String::<20>::from(content_len as u64);

        self.header("Content-Length", &content_len_str)
    }

    pub fn content_type(&mut self, content_type: &str) -> &mut Self {
        self.header("Content-Type", content_type)
    }

    pub fn content_encoding(&mut self, content_encoding: &str) -> &mut Self {
        self.header("Content-Encoding", content_encoding)
    }

    pub fn transfer_encoding(&mut self, transfer_encoding: &str) -> &mut Self {
        self.header("Transfer-Encoding", transfer_encoding)
    }

    pub fn payload(&self) -> &[u8] {
        self.0.payload()
    }

    pub fn release(self) -> &'b mut [u8] {
        self.0.release()
    }
}

pub struct Response<'b, const N: usize> {
    pub version: Option<u8>,
    pub code: Option<u16>,
    pub reason: Option<&'b str>,
    pub headers: [Header<'b>; N],
}

impl<'b, const N: usize> Response<'b, N> {
    pub async fn parse<R>(
        mut input: R,
        buf: &'b mut [u8],
    ) -> Result<(Response<'b, N>, Body<'b, R>), Error<R::Error>>
    where
        R: Read,
    {
        let mut headers = [EMPTY_HEADER; N];

        let mut response = httparse::Response::new(&mut headers);

        let read_len = io::try_read_full(&mut input, buf)
            .await
            .map_err(|(e, _)| Error::Read(e))?;

        let status = response.parse(&buf[..read_len])?;

        if let Status::Complete(response_len) = status {
            let response = Self {
                version: response.version,
                code: response.code,
                reason: response.reason,
                headers,
            };

            let response_body = Body {
                buf: &buf[response_len..read_len],
                content_len: response.content_len().unwrap_or(usize::MAX),
                read_len: 0,
                input,
            };

            Ok((response, response_body))
        } else {
            Err(Error::TooManyHeaders)
        }
    }

    pub fn status_code(&self) -> u16 {
        self.code.unwrap_or(200)
    }

    pub fn status_message(&self) -> Option<&str> {
        self.reason
    }

    pub fn content_len(&self) -> Option<usize> {
        self.header("Content-Length")
            .map(|content_len_str| content_len_str.parse::<usize>().unwrap())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header("Content-Type")
    }

    pub fn content_encoding(&self) -> Option<&str> {
        self.header("Content-Encoding")
    }

    pub fn transfer_encoding(&self) -> Option<&str> {
        self.header("Transfer-Encoding")
    }

    pub fn headers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers_raw()
            .map(|(name, value)| (name, unsafe { str::from_utf8_unchecked(value) }))
    }

    pub fn headers_raw(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.headers
            .iter()
            .map(|header| (header.name, header.value))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers()
            .find(|(hname, _)| UncasedStr::new(name) == UncasedStr::new(hname))
            .map(|(_, value)| value)
    }

    pub fn header_raw(&self, name: &str) -> Option<&[u8]> {
        self.headers_raw()
            .find(|(hname, _)| UncasedStr::new(name) == UncasedStr::new(hname))
            .map(|(_, value)| value)
    }
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;

    use embedded_svc::http::client::asynch::Method;
    use embedded_svc::io::asynch::{Io, Read, Write};

    use embedded_nal_async::TcpClientSocket;

    pub struct Client<'b, const N: usize, T>
    where
        T: TcpClientSocket + 'b,
    {
        buf: &'b mut [u8],
        tcp_client: T,
    }

    impl<'b, const N: usize, T> Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        pub fn new(buf: &'b mut [u8], tcp_client: T) -> Self {
            Self { buf, tcp_client }
        }
    }

    impl<'b, const N: usize, T> embedded_svc::io::asynch::Io for Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        type Error = T::Error;
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Client for Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        type Request<'a>
        where
            Self: 'a,
        = ClientRequest<'a, N, &'a mut T>;

        type RequestFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<Self::Request<'a>, Self::Error>>;

        fn request<'a>(&'a mut self, method: Method, uri: &'a str) -> Self::RequestFuture<'a> {
            async move {
                // TODO: We need a no_std URI parser

                self.tcp_client
                    .connect("1.1.1.1:80".parse().unwrap())
                    .await?;

                Ok(Self::Request::new(
                    method,
                    uri,
                    self.buf,
                    &mut self.tcp_client,
                ))
            }
        }
    }

    impl From<Method> for super::Method {
        fn from(method: Method) -> Self {
            match method {
                Method::Delete => super::Method::Delete,
                Method::Get => super::Method::Get,
                Method::Head => super::Method::Head,
                Method::Post => super::Method::Post,
                Method::Put => super::Method::Put,
                Method::Connect => super::Method::Connect,
                Method::Options => super::Method::Options,
                Method::Trace => super::Method::Trace,
                Method::Copy => super::Method::Copy,
                Method::Lock => super::Method::Lock,
                Method::MkCol => super::Method::MkCol,
                Method::Move => super::Method::Move,
                Method::Propfind => super::Method::Propfind,
                Method::Proppatch => super::Method::Proppatch,
                Method::Search => super::Method::Search,
                Method::Unlock => super::Method::Unlock,
                Method::Bind => super::Method::Bind,
                Method::Rebind => super::Method::Rebind,
                Method::Unbind => super::Method::Unbind,
                Method::Acl => super::Method::Acl,
                Method::Report => super::Method::Report,
                Method::MkActivity => super::Method::MkActivity,
                Method::Checkout => super::Method::Checkout,
                Method::Merge => super::Method::Merge,
                Method::MSearch => super::Method::MSearch,
                Method::Notify => super::Method::Notify,
                Method::Subscribe => super::Method::Subscribe,
                Method::Unsubscribe => super::Method::Unsubscribe,
                Method::Patch => super::Method::Patch,
                Method::Purge => super::Method::Purge,
                Method::MkCalendar => super::Method::MkCalendar,
                Method::Link => super::Method::Link,
                Method::Unlink => super::Method::Unlink,
            }
        }
    }

    impl<'b> embedded_svc::http::client::asynch::SendHeaders for super::Request<'b> {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.header(name, value)
        }
    }

    pub struct ClientRequest<'b, const N: usize, T> {
        req_headers: super::Request<'b>,
        io: T,
    }

    impl<'b, const N: usize, T> ClientRequest<'b, N, T> {
        pub fn new(
            method: embedded_svc::http::client::asynch::Method,
            uri: &str,
            buf: &'b mut [u8],
            io: T,
        ) -> Self
        where
            T: Read + Write,
        {
            Self {
                req_headers: super::Request::new(method.into(), uri, buf),
                io,
            }
        }
    }

    impl<'b, const N: usize, T> Io for ClientRequest<'b, N, T>
    where
        T: Io,
    {
        type Error = T::Error;
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::SendHeaders
        for ClientRequest<'b, N, T>
    {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.req_headers.set_header(name, value);
            self
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Request for ClientRequest<'b, N, T>
    where
        T: Read + Write,
    {
        type Write = ClientRequestWrite<'b, N, T>;

        type IntoWriterFuture =
            impl Future<Output = Result<ClientRequestWrite<'b, N, T>, Self::Error>>;

        type SubmitFuture = impl Future<Output = Result<ClientResponse<'b, N, T>, Self::Error>>;

        fn into_writer(mut self) -> Self::IntoWriterFuture
        where
            Self: Sized,
        {
            async move {
                self.io.write_all(self.req_headers.payload()).await?;

                Ok(ClientRequestWrite {
                    buf: self.req_headers.release(),
                    io: self.io,
                })
            }
        }

        fn submit(self) -> Self::SubmitFuture
        where
            Self: Sized,
        {
            use embedded_svc::http::client::asynch::RequestWrite;

            async move { self.into_writer().await?.into_response().await }
        }
    }

    pub struct ClientRequestWrite<'b, const N: usize, T> {
        buf: &'b mut [u8],
        io: T,
    }

    impl<'b, const N: usize, T> Io for ClientRequestWrite<'b, N, T>
    where
        T: Io,
    {
        type Error = T::Error;
    }

    impl<'b, const N: usize, T> Write for ClientRequestWrite<'b, N, T>
    where
        T: Write,
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
        T: Read + Write,
    {
        type Response = ClientResponse<'b, N, T>;

        type IntoResponseFuture = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn into_response(mut self) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                self.io.flush().await?;

                let (response, body) = super::Response::parse(self.io, self.buf).await.unwrap(); // TODO

                Ok(Self::Response { response, body })
            }
        }
    }

    impl<'b, const N: usize> embedded_svc::http::client::asynch::Status for super::Response<'b, N> {
        fn status(&self) -> u16 {
            super::Response::status_code(self)
        }

        fn status_message(&self) -> Option<&'_ str> {
            super::Response::status_message(self)
        }
    }

    impl<'b, const N: usize> embedded_svc::http::client::asynch::Headers for super::Response<'b, N> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            super::Response::header(self, name)
        }
    }

    pub struct ClientResponse<'b, const N: usize, R> {
        response: super::Response<'b, N>,
        body: super::Body<'b, R>,
    }

    impl<'b, const N: usize, R> embedded_svc::http::client::asynch::Status
        for ClientResponse<'b, N, R>
    {
        fn status(&self) -> u16 {
            self.response.status_code()
        }

        fn status_message(&self) -> Option<&'_ str> {
            self.response.status_message()
        }
    }

    impl<'b, const N: usize, R> embedded_svc::http::client::asynch::Headers
        for ClientResponse<'b, N, R>
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.response.header(name)
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
        R: Read,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move { self.body.read(buf).await }
        }
    }

    impl<'b, const N: usize, R> embedded_svc::http::client::asynch::Response
        for ClientResponse<'b, N, R>
    where
        R: Read,
    {
        type Headers = super::Response<'b, N>;

        type Body = super::Body<'b, R>;

        fn split(self) -> (Self::Headers, Self::Body)
        where
            Self: Sized,
        {
            (self.response, self.body)
        }
    }
}
