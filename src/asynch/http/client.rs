use core::cmp::min;
use core::future::Future;
use core::str;

use embedded_io::{
    asynch::{Read, Write},
    Io,
};
use httparse::Status;
use uncased::UncasedStr;

use crate::asynch::io;
use crate::asynch::io::CopyError;

use super::*;

pub struct Request<'b, W> {
    headers: HttpSendHeaders<'b>,
    output: W,
}

impl<'b, W> Io for Request<'b, W>
where
    W: Io,
{
    type Error = W::Error;
}

impl<'b, W> Request<'b, W>
where
    W: Write,
{
    pub fn get(uri: &str, write: W, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Get, uri, write, buf)
    }

    pub fn post(uri: &str, write: W, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Post, uri, write, buf)
    }

    pub fn put(uri: &str, write: W, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Put, uri, write, buf)
    }

    pub fn delete(uri: &str, write: W, buf: &'b mut [u8]) -> Self {
        Self::new(Method::Delete, uri, write, buf)
    }

    pub fn new(method: Method, uri: &str, write: W, buf: &'b mut [u8]) -> Self {
        let mut this = Self {
            headers: HttpSendHeaders::new(buf),
            output: write,
        };

        this.headers
            .set_status_tokens(&[method.as_str(), "HTTP/1.1", uri]);

        this
    }

    pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
        self.headers.set(name, value);
        self
    }

    pub fn header_raw(&mut self, name: &str, value: &[u8]) -> &mut Self {
        self.headers.set_raw(name, value);
        self
    }

    pub async fn send_bytes<'a>(self, bytes: &'a [u8]) -> Result<W, W::Error>
    where
        Self: Sized + 'a,
    {
        let mut writer = self.into_writer().await?;

        writer.write_all(bytes).await?;

        Ok(writer)
    }

    pub async fn send_str<'a>(self, s: &'a str) -> Result<W, W::Error>
    where
        Self: Sized + 'a,
    {
        self.send_bytes(s.as_bytes()).await
    }

    #[allow(clippy::type_complexity)]
    pub async fn send_reader<R>(
        self,
        size: usize,
        read: R,
    ) -> Result<W, CopyError<R::Error, W::Error>>
    where
        R: Read,
        Self: Sized,
    {
        let mut write = self.into_writer().await.map_err(CopyError::Write)?;

        io::copy_len::<64, _, _>(read, &mut write, size as u64).await?;

        Ok(write)
    }

    pub async fn into_writer(mut self) -> Result<W, W::Error> {
        self.output.write_all(self.headers.buf()).await?;

        Ok(self.output)
    }
}

pub struct ResponseHeaders<'b, const N: usize>([httparse::Header<'b>; N]);

impl<'b, const N: usize> ResponseHeaders<'b, N> {
    pub fn new() -> Self {
        Self([httparse::EMPTY_HEADER; N])
    }
}

pub struct Response<'b, 'h, const N: usize>(httparse::Response<'b, 'h>);

pub struct ResponseBody<'b, R> {
    buf: &'b [u8],
    content_len: usize,
    read_len: usize,
    input: R,
}

impl<'b, 'h, const N: usize> Response<'b, 'h, N>
where
    'h: 'b,
{
    pub async fn parse<R>(
        mut input: R,
        buf: &'b mut [u8],
        headers: &'h mut ResponseHeaders<'b, N>,
    ) -> Result<(Response<'b, 'h, N>, ResponseBody<'b, R>), Error<R::Error>>
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

            let response_body = ResponseBody {
                buf: &buf[response_len..read_len],
                content_len: usize::MAX, // TODO
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

impl<'b, R> Io for ResponseBody<'b, R>
where
    R: Io,
{
    type Error = R::Error;
}

impl<'b, R> Read for ResponseBody<'b, R>
where
    R: Read,
{
    type ReadFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move {
            if self.buf.len() > self.read_len {
                let len = min(buf.len(), self.buf.len() - self.read_len);
                buf[..len].copy_from_slice(&self.buf[self.read_len..self.read_len + len]);

                self.read_len += len;

                Ok(len)
            } else {
                let len = min(buf.len(), self.content_len - self.read_len);
                if len > 0 {
                    let read = self.input.read(&mut buf[..len]).await?;
                    self.read_len += read;

                    Ok(read)
                } else {
                    Ok(0)
                }
            }
        }
    }
}
