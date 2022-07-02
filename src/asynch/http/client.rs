use core::str;

use embedded_io::asynch::Read;

use httparse::Status;
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

pub struct Response<'b, 'h, const N: usize>(httparse::Response<'h, 'b>);

impl<'b, 'h, const N: usize> Response<'b, 'h, N>
where
    'b: 'h,
{
    pub async fn parse<R>(
        mut input: R,
        buf: &'b mut [u8],
        headers: &'h mut Headers<'b, N>,
    ) -> Result<(Response<'b, 'h, N>, Body<'b, R>), Error<R::Error>>
    where
        R: Read,
    {
        let mut response = httparse::Response::new(&mut headers.0);

        let read_len = io::try_read_full(&mut input, buf)
            .await
            .map_err(|(e, _)| Error::Read(e))?;

        let status = response.parse(&buf[..read_len])?;

        if let Status::Complete(response_len) = status {
            let response = Self(response);

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
        self.0.code.unwrap_or(200)
    }

    pub fn status_message(&self) -> Option<&str> {
        self.0.reason
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
        self.0
            .headers
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

    use embedded_svc::io::asynch::{Io, Read, Write};

    use embedded_nal_async::TcpClient;

    pub struct Client<'b, const N: usize, T>
    where
        T: TcpClient + 'b,
    {
        buf: &'b mut [u8],
        resp_headers: super::Headers<'b, N>,
        tcp_client: &'b mut T,
        connection: Option<T::TcpConnection<'b>>,
    }

    impl<'b, const N: usize, T> Client<'b, N, T>
    where
        T: TcpClient + 'b,
    {
        pub fn new(buf: &'b mut [u8], tcp_client: &'b mut T) -> Self {
            Self {
                buf,
                resp_headers: super::Headers::<N>::new(),
                tcp_client,
                connection: None,
            }
        }
    }

    impl<'b, const N: usize, T> embedded_svc::io::asynch::Io for Client<'b, N, T>
    where
        T: TcpClient,
    {
        type Error = T::Error;
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Client for Client<'b, N, T>
    where
        T: TcpClient + 'b,
    {
        type Request<'a>
        where
            Self: 'a,
        = ClientRequest<
            'b,
            'a,
            N,
            <T as TcpClient>::TcpConnection<'a>,
            <T as TcpClient>::TcpConnection<'a>,
        >;

        type RequestFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<Self::Request<'a>, Self::Error>>;

        fn request<'a>(
            &'a mut self,
            method: embedded_svc::http::Method,
            uri: &str,
        ) -> Self::RequestFuture<'a> {
            // TODO: Logic to recycle the existing connection if it is still open and is for the same host
            self.connection = None;

            async move {
                // TODO: We need a no_std URI parser
                //self.connection = Some(self.tcp_client.connect("1.1.1.1:80".parse().unwrap()).await?);
                let connection: <T as TcpClient>::TcpConnection<'a> = self
                    .tcp_client
                    .connect("1.1.1.1:80".parse().unwrap())
                    .await?;

                let buf: &'a mut [u8] = self.buf;
                let resp_headers: &'a mut super::Headers<'b, N> = &mut self.resp_headers;
                let connection: Option<<T as TcpClient>::TcpConnection<'a>> = None;
                let connection2: Option<<T as TcpClient>::TcpConnection<'a>> = None;

                //Ok(Self::Request::new(method, uri, buf, resp_headers, connection.unwrap(), connection2.unwrap()))
                todo!()
            }
        }
    }

    impl From<embedded_svc::http::client::asynch::Method> for super::Method {
        fn from(_: embedded_svc::http::client::asynch::Method) -> Self {
            todo!()
        }
    }

    impl<'b> embedded_svc::http::client::asynch::SendHeaders for super::Request<'b> {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.header(name, value)
        }
    }

    pub struct ClientRequest<'b, 'h, const N: usize, R, W> {
        req_headers: super::Request<'b>,
        resp_headers: &'h mut super::Headers<'b, N>,
        input: R,
        output: W,
    }

    impl<'b, 'h, const N: usize, R, W> ClientRequest<'b, 'h, N, R, W> {
        pub fn new(
            method: embedded_svc::http::client::asynch::Method,
            uri: &str,
            buf: &'b mut [u8],
            resp_headers: &'h mut super::Headers<'b, N>,
            input: R,
            output: W,
        ) -> Self
        where
            R: Read,
            W: Write<Error = R::Error>,
        {
            Self {
                req_headers: super::Request::new(method.into(), uri, buf),
                resp_headers,
                input,
                output,
            }
        }
    }

    impl<'b, 'h, const N: usize, R, W> Io for ClientRequest<'b, 'h, N, R, W>
    where
        W: Io,
    {
        type Error = W::Error;
    }

    impl<'b, 'h, const N: usize, R, W> embedded_svc::http::client::asynch::SendHeaders
        for ClientRequest<'b, 'h, N, R, W>
    {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.req_headers.header(name, value);
            self
        }
    }

    impl<'b, 'h, const N: usize, R, W> embedded_svc::http::client::asynch::Request
        for ClientRequest<'b, 'h, N, R, W>
    where
        'b: 'h,
        R: Read<Error = Self::Error>,
        W: Write,
    {
        type Write = ClientRequestWrite<'b, 'h, N, R, W>;

        type IntoWriterFuture =
            impl Future<Output = Result<ClientRequestWrite<'b, 'h, N, R, W>, Self::Error>>;

        type SubmitFuture = impl Future<Output = Result<ClientResponse<'b, 'h, N, R>, Self::Error>>;

        fn into_writer(mut self) -> Self::IntoWriterFuture
        where
            Self: Sized,
        {
            async move {
                self.output.write_all(self.req_headers.payload()).await?;

                Ok(ClientRequestWrite {
                    buf: self.req_headers.release(),
                    resp_headers: self.resp_headers,
                    input: self.input,
                    output: self.output,
                })
            }
        }

        fn submit(self) -> Self::SubmitFuture
        where
            Self: Sized,
        {
            use embedded_svc::http::client::asynch::RequestWrite;

            async move { Ok(self.into_writer().await?.into_response().await?) }
        }
    }

    pub struct ClientRequestWrite<'b, 'h, const N: usize, R, W> {
        buf: &'b mut [u8],
        resp_headers: &'h mut super::Headers<'b, N>,
        input: R,
        output: W,
    }

    impl<'b, 'h, const N: usize, R, W> Io for ClientRequestWrite<'b, 'h, N, R, W>
    where
        W: Io,
    {
        type Error = W::Error;
    }

    impl<'b, 'h, const N: usize, R, W> Write for ClientRequestWrite<'b, 'h, N, R, W>
    where
        W: Write,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = W::WriteFuture<'a>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            self.output.write(buf)
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = W::FlushFuture<'a>;

        fn flush<'a>(&'a mut self) -> Self::FlushFuture<'a> {
            self.output.flush()
        }
    }

    impl<'b, 'h, const N: usize, R, W> embedded_svc::http::client::asynch::RequestWrite
        for ClientRequestWrite<'b, 'h, N, R, W>
    where
        'b: 'h,
        W: Write,
        R: Read<Error = W::Error>,
    {
        type Response = ClientResponse<'b, 'h, N, R>;

        type IntoResponseFuture = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn into_response(mut self) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                self.output.flush().await?;

                let (response, body) =
                    super::Response::parse(self.input, self.buf, self.resp_headers)
                        .await
                        .unwrap(); // TODO

                Ok(Self::Response { response, body })
            }
        }
    }

    impl<'b, 'h, const N: usize> embedded_svc::http::client::asynch::Status
        for super::Response<'b, 'h, N>
    {
        fn status(&self) -> u16 {
            super::Response::status_code(self)
        }

        fn status_message(&self) -> Option<&'_ str> {
            super::Response::status_message(self)
        }
    }

    impl<'b, 'h, const N: usize> embedded_svc::http::client::asynch::Headers
        for super::Response<'b, 'h, N>
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            super::Response::header(self, name)
        }
    }

    pub struct ClientResponse<'b, 'h, const N: usize, R> {
        response: super::Response<'b, 'h, N>,
        body: super::Body<'b, R>,
    }

    impl<'b, 'h, const N: usize, R> embedded_svc::http::client::asynch::Status
        for ClientResponse<'b, 'h, N, R>
    {
        fn status(&self) -> u16 {
            self.response.status_code()
        }

        fn status_message(&self) -> Option<&'_ str> {
            self.response.status_message()
        }
    }

    impl<'b, 'h, const N: usize, R> embedded_svc::http::client::asynch::Headers
        for ClientResponse<'b, 'h, N, R>
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.response.header(name)
        }
    }

    impl<'b, 'h, const N: usize, R> embedded_svc::io::Io for ClientResponse<'b, 'h, N, R>
    where
        R: Io,
    {
        type Error = R::Error;
    }

    impl<'b, 'h, const N: usize, R> embedded_svc::io::asynch::Read for ClientResponse<'b, 'h, N, R>
    where
        'b: 'h,
        R: Read,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move { Ok(self.body.read(buf).await?) }
        }
    }

    impl<'b, 'h, const N: usize, R> embedded_svc::http::client::asynch::Response
        for ClientResponse<'b, 'h, N, R>
    where
        'b: 'h,
        R: Read,
    {
        type Headers = super::Response<'b, 'h, N>;

        type Body = super::Body<'b, R>;

        fn split(self) -> (Self::Headers, Self::Body)
        where
            Self: Sized,
        {
            (self.response, self.body)
        }
    }
}
