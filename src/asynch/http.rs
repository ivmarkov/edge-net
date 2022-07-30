use core::cmp::min;
use core::fmt::{Display, Write as _};
use core::future::Future;
use core::str;

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;

use httparse::{Header, Status, EMPTY_HEADER};
use log::trace;
use uncased::UncasedStr;

use crate::close::Close;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

use super::ws::http::UpgradeError;

pub mod client;
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
    InvalidState,
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
            Self::TooLongHeaders => write!(f, "HTTP headers section is too long"),
            Self::TooLongBody => write!(f, "HTTP body is too long"),
            Self::IncompleteHeaders => write!(f, "HTTP headers section is incomplete"),
            Self::IncompleteBody => write!(f, "HTTP body is incomplete"),
            Self::InvalidState => write!(f, "Connection is not in requested state"),
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

impl Display for Method {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub async fn send_request<W>(
    method: Option<Method>,
    path: Option<&str>,
    output: W,
) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    send_status_line(method.map(|method| method.as_str()), path, output).await
}

pub async fn send_status<W>(
    status: Option<u16>,
    reason: Option<&str>,
    output: W,
) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    let status_str = status.map(heapless::String::<5>::from);

    send_status_line(
        status_str.as_ref().map(|status| status.as_str()),
        reason,
        output,
    )
    .await
}

pub async fn send_headers<'a, H, W>(headers: H, output: W) -> Result<BodyType, Error<W::Error>>
where
    W: Write,
    H: IntoIterator<Item = &'a (&'a str, &'a str)>,
{
    send_raw_headers(
        headers
            .into_iter()
            .map(|(name, value)| (*name, value.as_bytes())),
        output,
    )
    .await
}

pub async fn send_raw_headers<'a, H, W>(
    headers: H,
    mut output: W,
) -> Result<BodyType, Error<W::Error>>
where
    W: Write,
    H: IntoIterator<Item = (&'a str, &'a [u8])>,
{
    let mut body = BodyType::Unknown;

    for (name, value) in headers.into_iter() {
        if body == BodyType::Unknown {
            body = BodyType::from_header(name, unsafe { str::from_utf8_unchecked(value) });
        }

        output.write_all(name.as_bytes()).await.map_err(Error::Io)?;
        output.write_all(b": ").await.map_err(Error::Io)?;
        output.write_all(value).await.map_err(Error::Io)?;
        output.write_all(b"\r\n").await.map_err(Error::Io)?;
    }

    Ok(body)
}

pub async fn send_headers_end<W>(mut output: W) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    output.write_all(b"\r\n").await.map_err(Error::Io)
}

#[derive(Debug)]
pub struct Headers<'b, const N: usize>([httparse::Header<'b>; N]);

impl<'b, const N: usize> Headers<'b, N> {
    pub const fn new() -> Self {
        Self([httparse::EMPTY_HEADER; N])
    }

    pub fn content_len(&self) -> Option<u64> {
        self.get("Content-Length")
            .map(|content_len_str| content_len_str.parse::<u64>().unwrap())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.get("Content-Type")
    }

    pub fn content_encoding(&self) -> Option<&str> {
        self.get("Content-Encoding")
    }

    pub fn transfer_encoding(&self) -> Option<&str> {
        self.get("Transfer-Encoding")
    }

    pub fn connection(&self) -> Option<&str> {
        self.get("Connection")
    }

    pub fn cache_control(&self) -> Option<&'_ str> {
        self.get("Cache-Control")
    }

    pub fn upgrade(&self) -> Option<&'_ str> {
        self.get("Upgrade")
    }

    pub fn is_ws_upgrade_request(&self) -> bool {
        crate::asynch::ws::http::is_upgrade_request(self.iter())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.iter_raw()
            .map(|(name, value)| (name, unsafe { str::from_utf8_unchecked(value) }))
    }

    pub fn iter_raw(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.0
            .iter()
            .filter(|header| !header.name.is_empty())
            .map(|header| (header.name, header.value))
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.iter()
            .find(|(hname, _)| UncasedStr::new(name) == UncasedStr::new(hname))
            .map(|(_, value)| value)
    }

    pub fn get_raw(&self, name: &str) -> Option<&[u8]> {
        self.iter_raw()
            .find(|(hname, _)| UncasedStr::new(name) == UncasedStr::new(hname))
            .map(|(_, value)| value)
    }

    pub fn set(&mut self, name: &'b str, value: &'b str) -> &mut Self {
        self.set_raw(name, value.as_bytes())
    }

    pub fn set_raw(&mut self, name: &'b str, value: &'b [u8]) -> &mut Self {
        for header in &mut self.0 {
            if header.name.is_empty() || UncasedStr::new(header.name) == UncasedStr::new(name) {
                *header = Header { name, value };
                return self;
            }
        }

        panic!("No space left");
    }

    pub fn remove(&mut self, name: &str) -> &mut Self {
        let index = self
            .0
            .iter()
            .enumerate()
            .find(|(_, header)| UncasedStr::new(header.name) == UncasedStr::new(name));

        if let Some((mut index, _)) = index {
            while index < self.0.len() - 1 {
                self.0[index] = self.0[index + 1];

                index += 1;
            }

            self.0[index] = EMPTY_HEADER;
        }

        self
    }

    pub fn set_content_len(
        &mut self,
        content_len: u64,
        buf: &'b mut heapless::String<20>,
    ) -> &mut Self {
        *buf = heapless::String::<20>::from(content_len);

        self.set("Content-Length", buf.as_str())
    }

    pub fn set_content_type(&mut self, content_type: &'b str) -> &mut Self {
        self.set("Content-Type", content_type)
    }

    pub fn set_content_encoding(&mut self, content_encoding: &'b str) -> &mut Self {
        self.set("Content-Encoding", content_encoding)
    }

    pub fn set_transfer_encoding(&mut self, transfer_encoding: &'b str) -> &mut Self {
        self.set("Transfer-Encoding", transfer_encoding)
    }

    pub fn set_transfer_encoding_chunked(&mut self) -> &mut Self {
        self.set_transfer_encoding("Chunked")
    }

    pub fn set_connection(&mut self, connection: &'b str) -> &mut Self {
        self.set("Connection", connection)
    }

    pub fn set_connection_close(&mut self) -> &mut Self {
        self.set_connection("Close")
    }

    pub fn set_connection_keep_alive(&mut self) -> &mut Self {
        self.set_connection("Keep-Alive")
    }

    pub fn set_connection_upgrade(&mut self) -> &mut Self {
        self.set_connection("Upgrade")
    }

    pub fn set_cache_control(&mut self, cache: &'b str) -> &mut Self {
        self.set("Cache-Control", cache)
    }

    pub fn set_cache_control_no_cache(&mut self) -> &mut Self {
        self.set_cache_control("No-Cache")
    }

    pub fn set_upgrade(&mut self, upgrade: &'b str) -> &mut Self {
        self.set("Upgrade", upgrade)
    }

    pub fn set_upgrade_websocket(&mut self) -> &mut Self {
        self.set_upgrade("websocket")
    }

    pub fn set_ws_upgrade_request_headers(
        &mut self,
        version: Option<&'b str>,
        nonce: &[u8; crate::asynch::ws::http::NONCE_LEN],
        nonce_base64_buf: &'b mut [u8; crate::asynch::ws::http::MAX_BASE64_KEY_LEN],
    ) -> &mut Self {
        for (name, value) in
            crate::asynch::ws::http::upgrade_request_headers(version, nonce, nonce_base64_buf)
        {
            self.set(name, value);
        }

        self
    }

    pub fn set_ws_upgrade_response_headers<'a, H>(
        &mut self,
        request_headers: H,
        version: Option<&'a str>,
        sec_key_response_base64_buf: &'b mut [u8; crate::asynch::ws::http::MAX_BASE64_KEY_RESPONSE_LEN],
    ) -> Result<&mut Self, UpgradeError>
    where
        H: IntoIterator<Item = (&'a str, &'a str)>,
    {
        for (name, value) in crate::asynch::ws::http::upgrade_response_headers(
            request_headers,
            version,
            sec_key_response_base64_buf,
        )? {
            self.set(name, value);
        }

        Ok(self)
    }

    pub async fn send<W>(&self, output: W) -> Result<BodyType, Error<W::Error>>
    where
        W: Write,
    {
        send_raw_headers(self.iter_raw(), output).await
    }
}

impl<'b, const N: usize> Default for Headers<'b, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BodyType {
    Chunked,
    ContentLen(u64),
    Close,
    Unknown,
}

impl BodyType {
    pub fn from_header(name: &str, value: &str) -> Self {
        if UncasedStr::new("Transfer-Encoding") == UncasedStr::new(name) {
            if UncasedStr::new(value) == UncasedStr::new("Chunked") {
                return Self::Chunked;
            }
        } else if UncasedStr::new("Content-Length") == UncasedStr::new(name) {
            return Self::ContentLen(value.parse::<u64>().unwrap()); // TODO
        } else if UncasedStr::new("Connection") == UncasedStr::new(name)
            && UncasedStr::new(value) == UncasedStr::new("Close")
        {
            return Self::Close;
        }

        Self::Unknown
    }

    pub fn from_headers<'a, H>(headers: H) -> Self
    where
        H: IntoIterator<Item = (&'a str, &'a str)>,
    {
        for (name, value) in headers {
            let body = Self::from_header(name, value);

            if body != Self::Unknown {
                return body;
            }
        }

        Self::Unknown
    }
}

pub enum Body<'b, R> {
    Close(PartiallyRead<'b, R>),
    ContentLen(ContentLenRead<PartiallyRead<'b, R>>),
    Chunked(ChunkedRead<'b, PartiallyRead<'b, R>>),
}

impl<'b, R> Body<'b, R>
where
    R: Read,
{
    pub fn new(body_type: BodyType, buf: &'b mut [u8], read_len: usize, input: R) -> Self {
        match body_type {
            BodyType::Chunked => Body::Chunked(ChunkedRead::new(
                PartiallyRead::new(&[], input),
                buf,
                read_len,
            )),
            BodyType::ContentLen(content_len) => Body::ContentLen(ContentLenRead::new(
                content_len,
                PartiallyRead::new(&buf[..read_len], input),
            )),
            BodyType::Close => Body::Close(PartiallyRead::new(&buf[..read_len], input)),
            BodyType::Unknown => Body::ContentLen(ContentLenRead::new(
                0,
                PartiallyRead::new(&buf[..read_len], input),
            )),
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
            Self::Close(r) => &mut r.input,
            Self::ContentLen(r) => &mut r.input.input,
            Self::Chunked(r) => &mut r.input.input,
        }
    }

    pub fn release(self) -> R {
        match self {
            Self::Close(r) => r.release(),
            Self::ContentLen(r) => r.release().release(),
            Self::Chunked(r) => r.release().release(),
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
    R: Read,
{
    type ReadFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

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

    pub fn buf_len(&self) -> usize {
        self.buf.len()
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
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

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
    content_len: u64,
    read_len: u64,
    input: R,
}

impl<R> ContentLenRead<R> {
    pub const fn new(content_len: u64, input: R) -> Self {
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
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move {
            let len = min(buf.len() as _, self.content_len - self.read_len);
            if len > 0 {
                let read = self
                    .input
                    .read(&mut buf[..len as _])
                    .await
                    .map_err(Error::Io)?;
                self.read_len += read as u64;

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
        &'a mut self,
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
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

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

pub enum SendBody<W> {
    Close(W),
    ContentLen(ContentLenWrite<W>),
    Chunked(ChunkedWrite<W>),
}

impl<W> SendBody<W>
where
    W: Write,
{
    pub fn new(body_type: BodyType, output: W) -> SendBody<W> {
        match body_type {
            BodyType::Chunked => SendBody::Chunked(ChunkedWrite::new(output)),
            BodyType::ContentLen(content_len) => {
                SendBody::ContentLen(ContentLenWrite::new(content_len, output))
            }
            BodyType::Close => SendBody::Close(output),
            BodyType::Unknown => SendBody::ContentLen(ContentLenWrite::new(0, output)),
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
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

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
    = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

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
    content_len: u64,
    write_len: u64,
    output: W,
}

impl<W> ContentLenWrite<W> {
    pub const fn new(content_len: u64, output: W) -> Self {
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
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move {
            if self.content_len > self.write_len + buf.len() as u64 {
                let write = self.output.write(buf).await.map_err(Error::Io)?;
                self.write_len += write as u64;

                Ok(write)
            } else {
                Err(Error::TooLongBody)
            }
        }
    }

    type FlushFuture<'a>
    = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

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
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

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
    = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move { self.output.flush().await.map_err(Error::Io) }
    }
}

#[derive(Default, Debug)]
pub struct RequestHeaders<'b, const N: usize> {
    pub method: Option<Method>,
    pub path: Option<&'b str>,
    pub headers: Headers<'b, N>,
}

impl<'b, const N: usize> RequestHeaders<'b, N> {
    pub const fn new() -> Self {
        Self {
            method: None,
            path: None,
            headers: Headers::<N>::new(),
        }
    }

    pub async fn receive<R>(
        &mut self,
        buf: &'b mut [u8],
        mut input: R,
    ) -> Result<(&'b mut [u8], usize), Error<R::Error>>
    where
        R: Read,
    {
        let (read_len, headers_len) = match read_reply_buf::<N, _>(&mut input, buf, true).await {
            Ok(read_len) => read_len,
            Err(e) => return Err(e),
        };

        let mut parser = httparse::Request::new(&mut self.headers.0);

        let (headers_buf, body_buf) = buf.split_at_mut(headers_len);

        let status = match parser.parse(headers_buf) {
            Ok(status) => status,
            Err(e) => return Err(e.into()),
        };

        if let Status::Complete(headers_len2) = status {
            if headers_len != headers_len2 {
                unreachable!("Should not happen. HTTP header parsing is indeterminate.")
            }

            self.method = parser.method.and_then(Method::new);
            self.path = parser.path;

            trace!("Received:\n{}", self);

            Ok((body_buf, read_len - headers_len))
        } else {
            unreachable!("Secondary parse of already loaded buffer failed.")
        }
    }

    pub async fn send<W>(&self, mut output: W) -> Result<BodyType, Error<W::Error>>
    where
        W: Write,
    {
        send_request(self.method, self.path, &mut output).await?;
        let body_type = self.headers.send(&mut output).await?;
        send_headers_end(output).await?;

        Ok(body_type)
    }
}

impl<'b, const N: usize> Display for RequestHeaders<'b, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // if let Some(version) = self.version {
        //     writeln!(f, "Version {}", version)?;
        // }

        if let Some(method) = self.method {
            writeln!(f, "{} {}", method, self.path.unwrap_or(""))?;
        }

        for (name, value) in self.headers.iter() {
            if name.is_empty() {
                break;
            }

            writeln!(f, "{}: {}", name, value)?;
        }

        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct ResponseHeaders<'b, const N: usize> {
    pub code: Option<u16>,
    pub reason: Option<&'b str>,
    pub headers: Headers<'b, N>,
}

impl<'b, const N: usize> ResponseHeaders<'b, N> {
    pub const fn new() -> Self {
        Self {
            code: None,
            reason: None,
            headers: Headers::<N>::new(),
        }
    }

    pub async fn receive<R>(
        &mut self,
        buf: &'b mut [u8],
        mut input: R,
    ) -> Result<(&'b mut [u8], usize), Error<R::Error>>
    where
        R: Read,
    {
        let (read_len, headers_len) = read_reply_buf::<N, _>(&mut input, buf, false).await?;

        let mut parser = httparse::Response::new(&mut self.headers.0);

        let (headers_buf, body_buf) = buf.split_at_mut(headers_len);

        let status = parser.parse(headers_buf).map_err(Error::from)?;

        if let Status::Complete(headers_len2) = status {
            if headers_len != headers_len2 {
                unreachable!("Should not happen. HTTP header parsing is indeterminate.")
            }

            self.code = parser.code;
            self.reason = parser.reason;

            trace!("Received:\n{}", self);

            Ok((body_buf, read_len - headers_len))
        } else {
            unreachable!("Secondary parse of already loaded buffer failed.")
        }
    }

    pub async fn send<W>(&self, mut output: W) -> Result<BodyType, Error<W::Error>>
    where
        W: Write,
    {
        send_status(self.code, self.reason, &mut output).await?;
        let body_type = self.headers.send(&mut output).await?;
        send_headers_end(output).await?;

        Ok(body_type)
    }
}

impl<'b, const N: usize> Display for ResponseHeaders<'b, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // if let Some(version) = self.version {
        //     writeln!(f, "Version {}", version)?;
        // }

        if let Some(code) = self.code {
            writeln!(f, "{} {}", code, self.reason.unwrap_or(""))?;
        }

        for (name, value) in self.headers.iter() {
            if name.is_empty() {
                break;
            }

            writeln!(f, "{}: {}", name, value)?;
        }

        Ok(())
    }
}

async fn read_reply_buf<const N: usize, R>(
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

async fn send_status_line<W>(
    token: Option<&str>,
    extra: Option<&str>,
    mut output: W,
) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    if let Some(token) = token {
        output
            .write_all(token.as_bytes())
            .await
            .map_err(Error::Io)?;
    }

    output.write_all(b" ").await.map_err(Error::Io)?;

    if let Some(extra) = extra {
        output
            .write_all(extra.as_bytes())
            .await
            .map_err(Error::Io)?;
    }

    output
        .write_all(b" HTTP/1.1\r\n")
        .await
        .map_err(Error::Io)?;

    Ok(())
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::str;

    use embedded_svc::http::client::asynch::Method;

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

    impl From<super::Method> for Method {
        fn from(method: super::Method) -> Self {
            match method {
                super::Method::Delete => Method::Delete,
                super::Method::Get => Method::Get,
                super::Method::Head => Method::Head,
                super::Method::Post => Method::Post,
                super::Method::Put => Method::Put,
                super::Method::Connect => Method::Connect,
                super::Method::Options => Method::Options,
                super::Method::Trace => Method::Trace,
                super::Method::Copy => Method::Copy,
                super::Method::Lock => Method::Lock,
                super::Method::MkCol => Method::MkCol,
                super::Method::Move => Method::Move,
                super::Method::Propfind => Method::Propfind,
                super::Method::Proppatch => Method::Proppatch,
                super::Method::Search => Method::Search,
                super::Method::Unlock => Method::Unlock,
                super::Method::Bind => Method::Bind,
                super::Method::Rebind => Method::Rebind,
                super::Method::Unbind => Method::Unbind,
                super::Method::Acl => Method::Acl,
                super::Method::Report => Method::Report,
                super::Method::MkActivity => Method::MkActivity,
                super::Method::Checkout => Method::Checkout,
                super::Method::Merge => Method::Merge,
                super::Method::MSearch => Method::MSearch,
                super::Method::Notify => Method::Notify,
                super::Method::Subscribe => Method::Subscribe,
                super::Method::Unsubscribe => Method::Unsubscribe,
                super::Method::Patch => Method::Patch,
                super::Method::Purge => Method::Purge,
                super::Method::MkCalendar => Method::MkCalendar,
                super::Method::Link => Method::Link,
                super::Method::Unlink => Method::Unlink,
            }
        }
    }

    impl<'b, const N: usize> embedded_svc::http::Query for super::RequestHeaders<'b, N> {
        fn uri(&self) -> &'_ str {
            self.path.unwrap_or("")
        }

        fn method(&self) -> Method {
            self.method.unwrap_or(super::Method::Get).into()
        }
    }

    impl<'b, const N: usize> embedded_svc::http::Headers for super::RequestHeaders<'b, N> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.headers.get(name)
        }
    }

    impl<'b, const N: usize> embedded_svc::http::Status for super::ResponseHeaders<'b, N> {
        fn status(&self) -> u16 {
            self.code.unwrap_or(200)
        }

        fn status_message(&self) -> Option<&'_ str> {
            self.reason
        }
    }

    impl<'b, const N: usize> embedded_svc::http::Headers for super::ResponseHeaders<'b, N> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.headers.get(name)
        }
    }

    impl<'b, const N: usize> embedded_svc::http::Headers for super::Headers<'b, N> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.get(name)
        }
    }
}
