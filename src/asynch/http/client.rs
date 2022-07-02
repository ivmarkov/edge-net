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

    impl<'b> embedded_svc::http::client::asynch::SendHeaders for super::Request<'b> {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.header(name, value)
        }
    }

    pub struct ClientRequest<'b, 'h, const N: usize, R, W> {
        req_headers: crate::asynch::http::SendHeaders<'b>,
        resp_headers: &'h mut super::Headers<'b, N>,
        input: R,
        output: W,
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
            self.req_headers.set(name, value);
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
