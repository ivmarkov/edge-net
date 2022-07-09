use core::str;

use embedded_io::asynch::Read;

use httparse::Status;
use uncased::UncasedStr;

use crate::asynch::io;

use super::*;

pub struct Request<'b, 'h, const N: usize>(httparse::Request<'b, 'h>);

impl<'b, 'h, const N: usize> Request<'b, 'h, N>
where
    'h: 'b,
{
    pub async fn parse<R>(
        mut input: R,
        buf: &'b mut [u8],
        headers: &'h mut Headers<'b, N>,
    ) -> Result<(Request<'b, 'h, N>, Body<'b, R>), Error<R::Error>>
    where
        R: Read,
    {
        let mut request = httparse::Request::new(&mut headers.0);

        let read_len = io::try_read_full(&mut input, buf)
            .await
            .map_err(|(e, _)| Error::Read(e))?;

        let status = request.parse(&buf[..read_len])?;

        if let Status::Complete(request_len) = status {
            let request = Self(request);

            let body = Body {
                buf: &buf[request_len..read_len],
                content_len: request.content_len().unwrap_or(usize::MAX),
                read_len: 0,
                input,
            };

            Ok((request, body))
        } else {
            Err(Error::TooManyHeaders)
        }
    }

    pub fn method(&self) -> Option<Method> {
        self.0.method.and_then(Method::new)
    }

    pub fn method_str(&self) -> Option<&str> {
        self.0.method
    }

    pub fn uri(&self) -> Option<&str> {
        self.0.path
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

pub struct Response<'b>(SendHeaders<'b>);

impl<'b> Response<'b> {
    pub fn ok(buf: &'b mut [u8]) -> Self {
        Self::new(200, None, buf)
    }

    pub fn new(status: u16, message: Option<&str>, buf: &'b mut [u8]) -> Self {
        let mut this = Self(SendHeaders::new(buf, &[]));

        this.status(status, message);

        this
    }

    pub fn status(&mut self, status: u16, message: Option<&str>) -> &mut Self {
        let status_str = heapless::String::<5>::from(status);

        if let Some(message) = message {
            self.0.set_status_tokens(&[&status_str, message]);
        } else {
            self.0.set_status_tokens(&[&status_str]);
        }

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

    pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
        self.0.set(name, value);
        self
    }

    pub fn header_raw(&mut self, name: &str, value: &[u8]) -> &mut Self {
        self.0.set_raw(name, value);
        self
    }

    pub fn payload(&mut self) -> &[u8] {
        self.0.payload()
    }
}
