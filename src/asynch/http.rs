use core::cmp::{max, min, Ordering};
use core::fmt::{Display, Write as _};
use core::future::Future;
use core::str;

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;

use log::trace;
use uncased::UncasedStr;

use crate::close::Close;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

pub mod client;
pub mod completion;
pub mod server;

/// An error in parsing the headers or the body.
#[derive(Debug)]
pub enum Error<E> {
    InvalidHeaders,
    InvalidBody,
    TooManyHeaders,
    TooLongHeaders,
    TooLongBody,
    IncompleteHeaders,
    IncompleteBody,
    Io(E),
}

impl<E> From<httparse::Error> for Error<E> {
    fn from(e: httparse::Error) -> Self {
        match e {
            httparse::Error::HeaderName => Self::InvalidHeaders,
            httparse::Error::HeaderValue => Self::InvalidHeaders,
            httparse::Error::NewLine => Self::InvalidHeaders,
            httparse::Error::Status => Self::InvalidHeaders,
            httparse::Error::Token => Self::InvalidHeaders,
            httparse::Error::TooManyHeaders => Self::TooManyHeaders,
            httparse::Error::Version => Self::InvalidHeaders,
        }
    }
}

impl<E> embedded_io::Error for Error<E>
where
    E: embedded_io::Error,
{
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Self::Io(e) => e.kind(),
            _ => embedded_io::ErrorKind::Other,
        }
    }
}

impl<E> Display for Error<E>
where
    E: Display,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidHeaders => write!(f, "Invalid HTTP headers or status line"),
            Self::InvalidBody => write!(f, "Invalid HTTP body"),
            Self::TooManyHeaders => write!(f, "Too many HTTP headers"),
            Self::TooLongHeaders => {
                write!(f, "HTTP headers section is too long")
            }
            Self::TooLongBody => write!(f, "HTTP body is too long"),
            Self::IncompleteHeaders => write!(f, "HTTP headers section is incomplete"),
            Self::IncompleteBody => write!(f, "HTTP body is incomplete"),
            Self::Io(e) => write!(f, "{}", e),
        }
    }
}

#[cfg(feature = "std")]
impl<E> std::error::Error for Error<E> where E: std::error::Error {}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Hash))]
pub enum Method {
    Delete,
    Get,
    Head,
    Post,
    Put,
    Connect,
    Options,
    Trace,
    Copy,
    Lock,
    MkCol,
    Move,
    Propfind,
    Proppatch,
    Search,
    Unlock,
    Bind,
    Rebind,
    Unbind,
    Acl,
    Report,
    MkActivity,
    Checkout,
    Merge,
    MSearch,
    Notify,
    Subscribe,
    Unsubscribe,
    Patch,
    Purge,
    MkCalendar,
    Link,
    Unlink,
}

impl Method {
    pub fn new(method: &str) -> Option<Self> {
        let method = UncasedStr::new(method);

        if method == UncasedStr::new("Delete") {
            Some(Self::Delete)
        } else if method == UncasedStr::new("Get") {
            Some(Self::Get)
        } else if method == UncasedStr::new("Head") {
            Some(Self::Head)
        } else if method == UncasedStr::new("Post") {
            Some(Self::Post)
        } else if method == UncasedStr::new("Put") {
            Some(Self::Put)
        } else if method == UncasedStr::new("Connect") {
            Some(Self::Connect)
        } else if method == UncasedStr::new("Options") {
            Some(Self::Options)
        } else if method == UncasedStr::new("Trace") {
            Some(Self::Trace)
        } else if method == UncasedStr::new("Copy") {
            Some(Self::Copy)
        } else if method == UncasedStr::new("Lock") {
            Some(Self::Lock)
        } else if method == UncasedStr::new("MkCol") {
            Some(Self::MkCol)
        } else if method == UncasedStr::new("Move") {
            Some(Self::Move)
        } else if method == UncasedStr::new("Propfind") {
            Some(Self::Propfind)
        } else if method == UncasedStr::new("Proppatch") {
            Some(Self::Proppatch)
        } else if method == UncasedStr::new("Search") {
            Some(Self::Search)
        } else if method == UncasedStr::new("Unlock") {
            Some(Self::Unlock)
        } else if method == UncasedStr::new("Bind") {
            Some(Self::Bind)
        } else if method == UncasedStr::new("Rebind") {
            Some(Self::Rebind)
        } else if method == UncasedStr::new("Unbind") {
            Some(Self::Unbind)
        } else if method == UncasedStr::new("Acl") {
            Some(Self::Acl)
        } else if method == UncasedStr::new("Report") {
            Some(Self::Report)
        } else if method == UncasedStr::new("MkActivity") {
            Some(Self::MkActivity)
        } else if method == UncasedStr::new("Checkout") {
            Some(Self::Checkout)
        } else if method == UncasedStr::new("Merge") {
            Some(Self::Merge)
        } else if method == UncasedStr::new("MSearch") {
            Some(Self::MSearch)
        } else if method == UncasedStr::new("Notify") {
            Some(Self::Notify)
        } else if method == UncasedStr::new("Subscribe") {
            Some(Self::Subscribe)
        } else if method == UncasedStr::new("Unsubscribe") {
            Some(Self::Unsubscribe)
        } else if method == UncasedStr::new("Patch") {
            Some(Self::Patch)
        } else if method == UncasedStr::new("Purge") {
            Some(Self::Purge)
        } else if method == UncasedStr::new("MkCalendar") {
            Some(Self::MkCalendar)
        } else if method == UncasedStr::new("Link") {
            Some(Self::Link)
        } else if method == UncasedStr::new("Unlink") {
            Some(Self::Unlink)
        } else {
            None
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Delete => "DELETE",
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Connect => "CONNECT",
            Self::Options => "OPTIONS",
            Self::Trace => "TRACE",
            Self::Copy => "COPY",
            Self::Lock => "LOCK",
            Self::MkCol => "MKCOL",
            Self::Move => "MOVE",
            Self::Propfind => "PROPFIND",
            Self::Proppatch => "PROPPATCH",
            Self::Search => "SEARCH",
            Self::Unlock => "UNLOCK",
            Self::Bind => "BIND",
            Self::Rebind => "REBIND",
            Self::Unbind => "UNBIND",
            Self::Acl => "ACL",
            Self::Report => "REPORT",
            Self::MkActivity => "MKACTIVITY",
            Self::Checkout => "CHECKOUT",
            Self::Merge => "MERGE",
            Self::MSearch => "MSEARCH",
            Self::Notify => "NOTIFY",
            Self::Subscribe => "SUBSCRIBE",
            Self::Unsubscribe => "UNSUBSCRIBE",
            Self::Patch => "PATCH",
            Self::Purge => "PURGE",
            Self::MkCalendar => "MKCALENDAR",
            Self::Link => "LINK",
            Self::Unlink => "UNLINK",
        }
    }
}

#[derive(Debug)]
pub struct Headers<'b, const N: usize>([httparse::Header<'b>; N]);

impl<'b, const N: usize> Headers<'b, N> {
    pub fn new() -> Self {
        Self([httparse::EMPTY_HEADER; N])
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

    pub fn connection(&self) -> Option<&str> {
        self.header("Connection")
    }

    pub fn headers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers_raw()
            .map(|(name, value)| (name, unsafe { str::from_utf8_unchecked(value) }))
    }

    pub fn headers_raw(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.0.iter().map(|header| (header.name, header.value))
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

impl<'b, const N: usize> Default for Headers<'b, N> {
    fn default() -> Self {
        Self::new()
    }
}

pub enum Body<'b, R> {
    Close(R),
    ContentLen(ContentLenRead<R>),
    Chunked(ChunkedRead<'b, R>),
}

impl<'b, R> Body<'b, R>
where
    R: Read,
{
    #[allow(clippy::type_complexity)]
    pub fn new<const N: usize>(
        headers: &Headers<'b, N>,
        buf: &'b mut [u8],
        read_len: usize,
        input: R,
    ) -> Body<'b, PartiallyRead<'b, R>> {
        if headers
            .transfer_encoding()
            .map(|value| UncasedStr::new(value) == UncasedStr::new("chunked"))
            .unwrap_or(false)
        {
            Body::Chunked(ChunkedRead::new(
                PartiallyRead::new(&[], input),
                buf,
                read_len,
            ))
        } else if let Some(content_len) = headers.content_len() {
            Body::ContentLen(ContentLenRead::new(
                content_len,
                PartiallyRead::new(&buf[..read_len], input),
            ))
        } else if headers
            .connection()
            .map(|value| UncasedStr::new(value) == UncasedStr::new("close"))
            .unwrap_or(false)
        {
            Body::Close(PartiallyRead::new(&buf[..read_len], input))
        } else {
            Body::ContentLen(ContentLenRead::new(
                0,
                PartiallyRead::new(&buf[..read_len], input),
            ))
        }
    }

    pub fn is_complete(&self) -> bool {
        match self {
            Self::Close(_) => true,
            Self::ContentLen(r) => r.is_complete(),
            Self::Chunked(r) => r.is_complete(),
        }
    }

    pub fn as_raw_reader(&mut self) -> &mut R {
        match self {
            Self::Close(r) => r,
            Self::ContentLen(r) => &mut r.input,
            Self::Chunked(r) => &mut r.input,
        }
    }

    pub fn release(self) -> R {
        match self {
            Self::Close(r) => r,
            Self::ContentLen(r) => r.release(),
            Self::Chunked(r) => r.release(),
        }
    }
}

impl<'b, R> Io for Body<'b, R>
where
    R: Io,
{
    type Error = Error<R::Error>;
}

impl<'b, R> Close for Body<'b, R>
where
    R: Close,
{
    fn close(&mut self) {
        match self {
            Self::Close(r) => r.close(),
            Self::ContentLen(r) => r.close(),
            Self::Chunked(r) => r.close(),
        }
    }
}

impl<'b, R> Read for Body<'b, R>
where
    R: Read + Close,
{
    type ReadFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move {
            match self {
                Self::Close(read) => Ok(read.read(buf).await.map_err(Error::Io)?),
                Self::ContentLen(read) => Ok(read.read(buf).await?),
                Self::Chunked(read) => Ok(read.read(buf).await?),
            }
        }
    }
}

pub struct PartiallyRead<'b, R> {
    buf: &'b [u8],
    read_len: usize,
    input: R,
}

impl<'b, R> PartiallyRead<'b, R> {
    pub const fn new(buf: &'b [u8], input: R) -> Self {
        Self {
            buf,
            read_len: 0,
            input,
        }
    }

    pub fn as_raw_reader(&mut self) -> &mut R {
        &mut self.input
    }

    pub fn release(self) -> R {
        self.input
    }
}

impl<'b, R> Io for PartiallyRead<'b, R>
where
    R: Io,
{
    type Error = R::Error;
}

impl<'b, R> Close for PartiallyRead<'b, R>
where
    R: Close,
{
    fn close(&mut self) {
        self.input.close()
    }
}

impl<'b, R> Read for PartiallyRead<'b, R>
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
                Ok(self.input.read(buf).await?)
            }
        }
    }
}

pub struct ContentLenRead<R> {
    content_len: usize,
    read_len: usize,
    input: R,
}

impl<R> ContentLenRead<R> {
    pub const fn new(content_len: usize, input: R) -> Self {
        Self {
            content_len,
            read_len: 0,
            input,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.content_len == self.read_len
    }

    pub fn release(self) -> R {
        self.input
    }
}

impl<R> Io for ContentLenRead<R>
where
    R: Io,
{
    type Error = Error<R::Error>;
}

impl<R> Close for ContentLenRead<R>
where
    R: Close,
{
    fn close(&mut self) {
        self.input.close()
    }
}

impl<R> Read for ContentLenRead<R>
where
    R: Read,
{
    type ReadFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move {
            let len = min(buf.len(), self.content_len - self.read_len);
            if len > 0 {
                let read = self.input.read(&mut buf[..len]).await.map_err(Error::Io)?;
                self.read_len += read;

                Ok(read)
            } else {
                Ok(0)
            }
        }
    }
}

pub struct ChunkedRead<'b, R> {
    buf: &'b mut [u8],
    buf_offset: usize,
    buf_len: usize,
    input: R,
    remain: u64,
    complete: bool,
}

impl<'b, R> ChunkedRead<'b, R>
where
    R: Read,
{
    pub fn new(input: R, buf: &'b mut [u8], buf_len: usize) -> Self {
        Self {
            buf,
            buf_offset: 0,
            buf_len,
            input,
            remain: 0,
            complete: false,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn release(self) -> R {
        self.input
    }

    // The elegant pull parser taken from here:
    // https://github.com/kchmck/uhttp_chunked_bytes.rs/blob/master/src/lib.rs
    // Changes:
    // - Converted to async
    // - Iterators removed
    // - Simpler error handling
    // - Consumption of trailer
    async fn next(&mut self) -> Result<Option<u8>, Error<R::Error>> {
        if self.complete {
            return Ok(None);
        }

        if self.remain == 0 {
            if let Some(size) = self.parse_size().await? {
                // If chunk size is zero (final chunk), the stream is finished [RFC7230§4.1].
                if size == 0 {
                    self.consume_trailer().await?;
                    self.complete = true;
                    return Ok(None);
                }

                self.remain = size;
            } else {
                self.complete = true;
                return Ok(None);
            }
        }

        let next = self.input_fetch().await?;
        self.remain -= 1;

        // If current chunk is finished, verify it ends with CRLF [RFC7230§4.1].
        if self.remain == 0 {
            self.consume_multi(b"\r\n").await?;
        }

        Ok(Some(next))
    }

    // Parse the number of bytes in the next chunk.
    async fn parse_size(&mut self) -> Result<Option<u64>, Error<R::Error>> {
        let mut digits = [0_u8; 16];

        let slice = match self.parse_digits(&mut digits[..]).await? {
            // This is safe because the following call to `from_str_radix` does
            // its own verification on the bytes.
            Some(s) => unsafe { std::str::from_utf8_unchecked(s) },
            None => return Ok(None),
        };

        let size = u64::from_str_radix(slice, 16).map_err(|_| Error::InvalidBody)?;

        Ok(Some(size))
    }

    // Extract the hex digits for the current chunk size.
    async fn parse_digits<'a>(
        &mut self,
        digits: &'a mut [u8],
    ) -> Result<Option<&'a [u8]>, Error<R::Error>> {
        // Number of hex digits that have been extracted.
        let mut len = 0;

        loop {
            let b = match self.input_next().await? {
                Some(b) => b,
                None => {
                    return if len == 0 {
                        // If EOF at the beginning of a new chunk, the stream is finished.
                        Ok(None)
                    } else {
                        Err(Error::IncompleteBody)
                    };
                }
            };

            match b {
                b'\r' => {
                    self.consume(b'\n').await?;
                    break;
                }
                b';' => {
                    self.consume_ext().await?;
                    break;
                }
                _ => {
                    match digits.get_mut(len) {
                        Some(d) => *d = b,
                        None => return Err(Error::InvalidBody),
                    }

                    len += 1;
                }
            }
        }

        Ok(Some(&digits[..len]))
    }

    // Consume and discard current chunk extension.
    // This doesn't check whether the characters up to CRLF actually have correct syntax.
    async fn consume_ext(&mut self) -> Result<(), Error<R::Error>> {
        self.consume_header().await?;

        Ok(())
    }

    // Consume and discard the optional trailer following the last chunk.
    async fn consume_trailer(&mut self) -> Result<(), Error<R::Error>> {
        while self.consume_header().await? {}

        Ok(())
    }

    // Consume and discard each header in the optional trailer following the last chunk.
    async fn consume_header(&mut self) -> Result<bool, Error<R::Error>> {
        let mut first = self.input_fetch().await?;
        let mut len = 1;

        loop {
            let second = self.input_fetch().await?;
            len += 1;

            if first == b'\r' && second == b'\n' {
                return Ok(len > 2);
            }

            first = second;
        }
    }

    // Verify the next bytes in the stream match the expectation.
    async fn consume_multi(&mut self, bytes: &[u8]) -> Result<(), Error<R::Error>> {
        for byte in bytes {
            self.consume(*byte).await?;
        }

        Ok(())
    }

    // Verify the next byte in the stream is matching the expectation.
    async fn consume(&mut self, byte: u8) -> Result<(), Error<R::Error>> {
        if self.input_fetch().await? == byte {
            Ok(())
        } else {
            Err(Error::InvalidBody)
        }
    }

    async fn input_fetch(&mut self) -> Result<u8, Error<R::Error>> {
        self.input_next().await?.ok_or(Error::IncompleteBody)
    }

    async fn input_next(&mut self) -> Result<Option<u8>, Error<R::Error>> {
        if self.buf_offset == self.buf_len {
            self.buf_len = self.input.read(self.buf).await.map_err(Error::Io)?;
            self.buf_offset = 0;
        }

        if self.buf_len > 0 {
            let byte = self.buf[self.buf_offset];
            self.buf_offset += 1;

            Ok(Some(byte))
        } else {
            Ok(None)
        }
    }
}

impl<'b, R> Close for ChunkedRead<'b, R>
where
    R: Close,
{
    fn close(&mut self) {
        self.input.close()
    }
}

impl<'b, R> Io for ChunkedRead<'b, R>
where
    R: Io,
{
    type Error = Error<R::Error>;
}

impl<'b, R> Read for ChunkedRead<'b, R>
where
    R: Read,
{
    type ReadFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move {
            for (index, byte_pos) in buf.iter_mut().enumerate() {
                if let Some(byte) = self.next().await? {
                    *byte_pos = byte;
                } else {
                    return Ok(index);
                }
            }

            Ok(buf.len())
        }
    }
}

struct SendHeadersSplitter<'a>(&'a [u8], usize);

impl<'a> SendHeadersSplitter<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self(buf, 0)
    }
}

impl<'a> Iterator for SendHeadersSplitter<'a> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let slice = &self.0[self.1..self.0.len()];

        if !slice.is_empty() {
            for index in 0..slice.len() - 1 {
                if slice[index..index + 2] == [13, 10] {
                    let result = (self.1, self.1 + index + 2);

                    self.1 = result.1;

                    return Some(result);
                }
            }
        }

        None
    }
}

#[derive(Debug)]
pub struct SendHeaders<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> SendHeaders<'a> {
    pub fn new(buf: &'a mut [u8], status_tokens: &[&str]) -> Self {
        let mut this = Self { buf, len: 0 };

        this.status(status_tokens);

        this
    }

    pub fn get_status(&self) -> &str {
        let end = self.get_status_len().unwrap();

        unsafe { str::from_utf8_unchecked(&self.buf[0..end - 2]) }
    }

    pub fn status(&mut self, tokens: &[&str]) -> &mut Self {
        if let Some(old_end) = self.get_status_len() {
            let new_end = tokens
                .iter()
                .map(|token| token.as_bytes().len())
                .sum::<usize>()
                + max(tokens.len(), 1)
                + 1; /* last separator is not a single space but \r\n */

            self.shift(old_end, new_end);

            let mut offset = 0;
            for (index, token) in tokens.iter().enumerate() {
                let bytes = token.as_bytes();

                self.buf[offset..offset + bytes.len()].copy_from_slice(bytes);
                offset += bytes.len();

                if index < tokens.len() - 1 {
                    self.buf[offset] = b' ';
                    offset += 1;
                }
            }

            self.buf[offset] = b'\r';
            self.buf[offset + 1] = b'\n';

            self.set_headers_end();
        } else {
            for (index, token) in tokens.iter().enumerate() {
                self.append(token.as_bytes());

                if index < tokens.len() - 1 {
                    self.append(b" ");
                }
            }

            self.append(b"\r\n");
            self.set_headers_end();
        }

        self
    }

    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.get_raw_header(name)
            .map(|value| unsafe { str::from_utf8_unchecked(value) })
    }

    pub fn get_raw_header(&self, name: &str) -> Option<&[u8]> {
        self.get_loc(name)
            .map(move |(start, end)| &self.buf[self.get_header_value_start(start, end)..end - 2])
    }

    pub fn header(&mut self, name: &str, value: &str) -> &mut Self {
        self.raw_header(name, value.as_bytes())
    }

    pub fn raw_header(&mut self, name: &str, value: &[u8]) -> &mut Self {
        if let Some((start, end)) = self.get_loc(name) {
            self.set_at(value, self.get_header_value_start(start, end), end - 2);
            self.set_headers_end();
        } else {
            self.append(name.as_bytes());
            self.append(b":");
            self.append(value);
            self.append(b"\r\n");
            self.set_headers_end();
        }

        self
    }

    pub fn remove(&mut self, name: &str) -> &mut Self {
        if let Some((start, end)) = self.get_loc(name) {
            self.shift(end, start);
            self.set_headers_end();
        }

        self
    }

    pub fn get_content_len(&self) -> Option<usize> {
        self.get_header("Content-Length")
            .map(|content_len_str| content_len_str.parse::<usize>().unwrap())
    }

    pub fn get_content_type(&self) -> Option<&str> {
        self.get_header("Content-Type")
    }

    pub fn get_content_encoding(&self) -> Option<&str> {
        self.get_header("Content-Encoding")
    }

    pub fn get_transfer_encoding(&self) -> Option<&str> {
        self.get_header("Transfer-Encoding")
    }

    pub fn get_connection(&self) -> Option<&str> {
        self.get_header("Connection")
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

    pub fn connection(&mut self, connection: &str) -> &mut Self {
        self.header("Connection", connection)
    }

    pub fn payload(&self) -> &[u8] {
        &self.buf[..self.len + 2]
    }

    pub fn release(self) -> &'a mut [u8] {
        self.buf
    }

    fn headers_payload(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn set_at(&mut self, value: &[u8], start: usize, end: usize) {
        self.shift(end, start + value.len());
        self.buf[start..start + value.len()].copy_from_slice(value);
    }

    fn append(&mut self, value: &[u8]) {
        self.check_space(self.len + value.len());

        self.buf[self.len..self.len + value.len()].copy_from_slice(value);
        self.len += value.len();
    }

    fn shift(&mut self, old_offset: usize, new_offset: usize) {
        match new_offset.cmp(&old_offset) {
            Ordering::Greater => {
                let delta = new_offset - old_offset;

                self.check_space(self.len + delta);

                for index in (new_offset..self.len + delta).rev() {
                    self.buf[index] = self.buf[index - delta];
                }

                self.len += delta;
            }
            Ordering::Less => {
                let delta = old_offset - new_offset;

                for index in new_offset..self.len - delta {
                    self.buf[index] = self.buf[index + delta];
                }

                self.len -= delta;
            }
            Ordering::Equal => {}
        }
    }

    fn get_loc(&self, name: &str) -> Option<(usize, usize)> {
        for (start, end) in SendHeadersSplitter::new(self.headers_payload()).skip(1) {
            let value_start = self.get_header_value_start(start, end);

            if UncasedStr::new(name)
                == UncasedStr::new(unsafe {
                    str::from_utf8_unchecked(&self.buf[start..value_start - 1])
                })
            {
                return Some((start, end));
            }
        }

        None
    }

    fn get_status_len(&self) -> Option<usize> {
        SendHeadersSplitter::new(self.headers_payload())
            .next()
            .map(|(_, end)| end)
    }

    fn get_header_value_start(&self, start: usize, end: usize) -> usize {
        let slice = &self.buf[start..end];

        for (index, ch) in slice.iter().enumerate() {
            if *ch == b':' {
                return start + index + 1;
            }
        }

        panic!("Malformed header");
    }

    fn set_headers_end(&mut self) {
        self.buf[self.len] = 13;
        self.buf[self.len + 1] = 10;
    }

    fn check_space(&self, len: usize) {
        if self.buf.len() < len + 2 {
            panic!("Buffer overflow. Please increase the size of the SendHeaders buffer.")
        }
    }
}

impl<'a> Display for SendHeaders<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (start, end) in SendHeadersSplitter::new(&self.buf[..self.len]) {
            write!(f, "{}", unsafe {
                str::from_utf8_unchecked(&self.buf[start..end])
            })?;
        }

        Ok(())
    }
}

pub enum SendBody<W> {
    Close(W),
    ContentLen(ContentLenWrite<W>),
    Chunked(ChunkedWrite<W>),
}

impl<W> SendBody<W>
where
    W: Write,
{
    #[allow(clippy::type_complexity)]
    pub fn new<'b>(headers: &'b SendHeaders<'b>, output: W) -> SendBody<W> {
        if headers
            .get_transfer_encoding()
            .map(|value| UncasedStr::new(value) == UncasedStr::new("chunked"))
            .unwrap_or(false)
        {
            SendBody::Chunked(ChunkedWrite::new(output))
        } else if let Some(content_len) = headers.get_content_len() {
            SendBody::ContentLen(ContentLenWrite::new(content_len, output))
        } else if headers
            .get_connection()
            .map(|value| UncasedStr::new(value) == UncasedStr::new("close"))
            .unwrap_or(false)
        {
            SendBody::Close(output)
        } else {
            SendBody::ContentLen(ContentLenWrite::new(0, output))
        }
    }

    pub fn is_complete(&self) -> bool {
        match self {
            Self::ContentLen(w) => w.is_complete(),
            _ => true,
        }
    }

    pub async fn finish(&mut self) -> Result<(), Error<W::Error>>
    where
        W: Write,
    {
        match self {
            Self::Close(_) => (),
            Self::ContentLen(_) => (),
            Self::Chunked(w) => w.finish().await?,
        }

        Ok(())
    }

    pub fn as_raw_writer(&mut self) -> &mut W {
        match self {
            Self::Close(w) => w,
            Self::ContentLen(w) => &mut w.output,
            Self::Chunked(w) => &mut w.output,
        }
    }

    pub fn release(self) -> W {
        match self {
            Self::Close(w) => w,
            Self::ContentLen(w) => w.release(),
            Self::Chunked(w) => w.release(),
        }
    }
}

impl<W> Io for SendBody<W>
where
    W: Io,
{
    type Error = Error<W::Error>;
}

impl<W> Close for SendBody<W>
where
    W: Close,
{
    fn close(&mut self) {
        match self {
            Self::Close(w) => w.close(),
            Self::ContentLen(w) => w.close(),
            Self::Chunked(w) => w.close(),
        }
    }
}

impl<W> Write for SendBody<W>
where
    W: Write,
{
    type WriteFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move {
            match self {
                Self::Close(w) => Ok(w.write(buf).await.map_err(Error::Io)?),
                Self::ContentLen(w) => Ok(w.write(buf).await?),
                Self::Chunked(w) => Ok(w.write(buf).await?),
            }
        }
    }

    type FlushFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<(), Self::Error>>;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move {
            match self {
                Self::Close(w) => Ok(w.flush().await.map_err(Error::Io)?),
                Self::ContentLen(w) => Ok(w.flush().await?),
                Self::Chunked(w) => Ok(w.flush().await?),
            }
        }
    }
}

pub struct ContentLenWrite<W> {
    content_len: usize,
    write_len: usize,
    output: W,
}

impl<W> ContentLenWrite<W> {
    pub const fn new(content_len: usize, output: W) -> Self {
        Self {
            content_len,
            write_len: 0,
            output,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.content_len == self.write_len
    }

    pub fn release(self) -> W {
        self.output
    }
}

impl<W> Io for ContentLenWrite<W>
where
    W: Io,
{
    type Error = Error<W::Error>;
}

impl<W> Close for ContentLenWrite<W>
where
    W: Close,
{
    fn close(&mut self) {
        self.output.close()
    }
}

impl<W> Write for ContentLenWrite<W>
where
    W: Write,
{
    type WriteFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move {
            if self.content_len > self.write_len + buf.len() {
                let write = self.output.write(buf).await.map_err(Error::Io)?;
                self.write_len += write;

                Ok(write)
            } else {
                Err(Error::TooLongBody)
            }
        }
    }

    type FlushFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<(), Self::Error>>;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move { self.output.flush().await.map_err(Error::Io) }
    }
}

pub struct ChunkedWrite<W> {
    output: W,
}

impl<W> ChunkedWrite<W> {
    pub const fn new(output: W) -> Self {
        Self { output }
    }

    pub async fn finish(&mut self) -> Result<(), Error<W::Error>>
    where
        W: Write,
    {
        self.output.write_all(b"\r\n").await.map_err(Error::Io)
    }

    pub fn release(self) -> W {
        self.output
    }
}

impl<W> Io for ChunkedWrite<W>
where
    W: Io,
{
    type Error = Error<W::Error>;
}

impl<W> Close for ChunkedWrite<W>
where
    W: Close,
{
    fn close(&mut self) {
        self.output.close()
    }
}

impl<W> Write for ChunkedWrite<W>
where
    W: Write,
{
    type WriteFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move {
            if !buf.is_empty() {
                let mut len_str = heapless::String::<10>::new();
                write!(&mut len_str, "{:X}\r\n", buf.len()).unwrap();
                self.output
                    .write_all(len_str.as_bytes())
                    .await
                    .map_err(Error::Io)?;

                self.output.write_all(buf).await.map_err(Error::Io)?;
                self.output
                    .write_all("\r\n".as_bytes())
                    .await
                    .map_err(Error::Io)?;

                Ok(buf.len())
            } else {
                Ok(0)
            }
        }
    }

    type FlushFuture<'a>
    where
        Self: 'a,
    = impl Future<Output = Result<(), Self::Error>>;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move { self.output.flush().await.map_err(Error::Io) }
    }
}

#[allow(clippy::needless_lifetimes)]
pub async fn send<'b, W>(
    headers: SendHeaders<'b>,
    mut output: W,
) -> Result<(&'b mut [u8], SendBody<W>), (W, W::Error)>
where
    W: Write,
{
    trace!("Sending:\n{}", headers);

    match output.write_all(headers.payload()).await {
        Ok(_) => match output.flush().await {
            Ok(_) => {
                let body = if headers
                    .get_transfer_encoding()
                    .map(|value| UncasedStr::new(value) == UncasedStr::new("chunked"))
                    .unwrap_or(false)
                {
                    SendBody::Chunked(ChunkedWrite::new(output))
                } else if let Some(content_len) = headers.get_content_len() {
                    SendBody::ContentLen(ContentLenWrite::new(content_len, output))
                } else if headers
                    .get_connection()
                    .map(|value| UncasedStr::new(value) == UncasedStr::new("close"))
                    .unwrap_or(false)
                {
                    SendBody::Close(output)
                } else {
                    SendBody::ContentLen(ContentLenWrite::new(0, output))
                };

                Ok((headers.release(), body))
            }
            Err(e) => Err((output, e)),
        },
        Err(e) => Err((output, e)),
    }
}

async fn receive_headers<const N: usize, R>(
    mut input: R,
    buf: &mut [u8],
    request: bool,
) -> Result<(usize, usize), Error<R::Error>>
where
    R: Read,
{
    let mut offset = 0;
    let mut size = 0;

    while buf.len() > size {
        let read = input.read(&mut buf[offset..]).await.map_err(Error::Io)?;

        offset += read;
        size += read;

        let mut headers = [httparse::EMPTY_HEADER; N];

        let status = if request {
            httparse::Request::new(&mut headers).parse(&buf[..size])?
        } else {
            httparse::Response::new(&mut headers).parse(&buf[..size])?
        };

        if let httparse::Status::Complete(headers_len) = status {
            return Ok((size, headers_len));
        }
    }

    Err(Error::TooManyHeaders)
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::str;

    use embedded_svc::http::client::asynch::Method;

    use crate::asynch::http::SendHeaders;

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

    impl<'b> embedded_svc::http::SendHeaders for SendHeaders<'b> {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.header(name, value)
        }
    }
}

#[test]
fn test() {
    fn compare_split(input: &str, outcome: &[&str]) {
        compare_split_buf(input.as_bytes(), outcome);
    }

    fn compare_split_buf(input: &[u8], outcome: &[&str]) {
        let mut splitter = SendHeadersSplitter::new(input);
        let mut outcome_splitter = outcome.iter();

        loop {
            let x = splitter.next();
            let y = outcome_splitter.next();

            assert_eq!(
                x.is_none(),
                y.is_none(),
                "Buf is {}, outcome is: {:?}",
                unsafe { str::from_utf8_unchecked(input) },
                outcome
            );

            if let Some((start, end)) = x {
                let x = unsafe { str::from_utf8_unchecked(&input[start..end]) };

                assert_eq!(
                    x,
                    *y.unwrap(),
                    "Buf is {}, outcome is: {:?}",
                    unsafe { str::from_utf8_unchecked(input) },
                    outcome
                );
            } else {
                break;
            }
        }
    }

    compare_split("", &[]);
    compare_split("foo", &[]);
    compare_split("foo\nbar\n", &[]);

    compare_split("foo\r\nbar\n", &["foo\r\n"]);
    compare_split("foo\r\nbar\n\r\r\n", &["foo\r\n", "bar\n\r\r\n"]);
    compare_split(
        "\r\n\r\nfoo\r\nbar\n\r\r\n\r\n",
        &["\r\n", "\r\n", "foo\r\n", "bar\n\r\r\n", "\r\n"],
    );

    let mut buf = [0_u8; 1024];

    let mut headers = SendHeaders::new(&mut buf, &[]);
    compare_split_buf(headers.headers_payload(), &["\r\n"]);

    headers.status(&["GET", "/ip", "HTTP/1.1"]);
    compare_split_buf(headers.headers_payload(), &["GET /ip HTTP/1.1\r\n"]);

    headers.status(&["GET", "/", "HTTP/1.1"]);
    compare_split_buf(headers.headers_payload(), &["GET / HTTP/1.1\r\n"]);

    headers.header("Content-Length", "42");
    headers.header("Content-Type", "text/html");
    compare_split_buf(
        headers.headers_payload(),
        &[
            "GET / HTTP/1.1\r\n",
            "Content-Length:42\r\n",
            "Content-Type:text/html\r\n",
        ],
    );

    headers.header("Content-Length", "0");
    compare_split_buf(
        headers.headers_payload(),
        &[
            "GET / HTTP/1.1\r\n",
            "Content-Length:0\r\n",
            "Content-Type:text/html\r\n",
        ],
    );

    headers.header("Content-Length", "65536");
    compare_split_buf(
        headers.headers_payload(),
        &[
            "GET / HTTP/1.1\r\n",
            "Content-Length:65536\r\n",
            "Content-Type:text/html\r\n",
        ],
    );

    headers.status(&["POST", "/foo", "HTTP/1.1"]);
    compare_split_buf(
        headers.headers_payload(),
        &[
            "POST /foo HTTP/1.1\r\n",
            "Content-Length:65536\r\n",
            "Content-Type:text/html\r\n",
        ],
    );

    assert_eq!(headers.get_status(), "POST /foo HTTP/1.1");
    assert_eq!(headers.get_header("Content-length"), Some("65536"));
    assert_eq!(headers.get_header("Content-Length1"), None);

    headers.remove("Content-Length");
    compare_split_buf(
        headers.headers_payload(),
        &["POST /foo HTTP/1.1\r\n", "Content-Type:text/html\r\n"],
    );

    assert_eq!(
        unsafe { str::from_utf8_unchecked(headers.payload()) },
        "POST /foo HTTP/1.1\r\nContent-Type:text/html\r\n\r\n"
    );
}
