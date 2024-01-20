use core::cmp::min;
use core::fmt::{Display, Write as _};
use core::str;

use embedded_io_async::{ErrorType, Read, Write};

use httparse::Status;

use log::trace;

use crate::ws::UpgradeError;
use crate::{BodyType, Headers, Method, RequestHeaders, ResponseHeaders};

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
    WsUpgradeError(UpgradeError),
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

impl<E> From<UpgradeError> for Error<E> {
    fn from(e: UpgradeError) -> Self {
        Self::WsUpgradeError(e)
    }
}

impl<E> embedded_io_async::Error for Error<E>
where
    E: embedded_io_async::Error,
{
    fn kind(&self) -> embedded_io_async::ErrorKind {
        match self {
            Self::Io(e) => e.kind(),
            _ => embedded_io_async::ErrorKind::Other,
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
            Self::WsUpgradeError(e) => write!(f, "WebSocket upgrade error: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(feature = "std")]
impl<E> std::error::Error for Error<E> where E: std::error::Error {}

impl<'b, const N: usize> RequestHeaders<'b, N> {
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

            self.http11 = if let Some(version) = parser.version {
                if version > 1 {
                    Err(Error::InvalidHeaders)?;
                }

                Some(version == 1)
            } else {
                None
            };

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
        send_request(
            self.http11.unwrap_or(false),
            self.method,
            self.path,
            &mut output,
        )
        .await?;
        let body_type = self.headers.send(&mut output).await?;
        send_headers_end(output).await?;

        Ok(body_type)
    }
}

impl<'b, const N: usize> ResponseHeaders<'b, N> {
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

            self.http11 = if let Some(version) = parser.version {
                if version > 1 {
                    Err(Error::InvalidHeaders)?;
                }

                Some(version == 1)
            } else {
                None
            };

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
        send_status(
            self.http11.unwrap_or(false),
            self.code,
            self.reason,
            &mut output,
        )
        .await?;
        let body_type = self.headers.send(&mut output).await?;
        send_headers_end(output).await?;

        Ok(body_type)
    }
}

pub(crate) async fn send_request<W>(
    http11: bool,
    method: Option<Method>,
    path: Option<&str>,
    output: W,
) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    send_status_line(
        true,
        http11,
        method.map(|method| method.as_str()),
        path,
        output,
    )
    .await
}

pub(crate) async fn send_status<W>(
    http11: bool,
    status: Option<u16>,
    reason: Option<&str>,
    output: W,
) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    let status_str: Option<heapless::String<5>> = status.map(|status| status.try_into().unwrap());

    send_status_line(
        false,
        http11,
        status_str.as_ref().map(|status| status.as_str()),
        reason,
        output,
    )
    .await
}

pub(crate) async fn send_headers<'a, H, W>(
    headers: H,
    output: W,
) -> Result<BodyType, Error<W::Error>>
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

pub(crate) async fn send_raw_headers<'a, H, W>(
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

pub(crate) async fn send_headers_end<W>(mut output: W) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    output.write_all(b"\r\n").await.map_err(Error::Io)
}

impl<'b, const N: usize> Headers<'b, N> {
    pub(crate) async fn send<W>(&self, output: W) -> Result<BodyType, Error<W::Error>>
    where
        W: Write,
    {
        send_raw_headers(self.iter_raw(), output).await
    }
}

#[allow(private_interfaces)]
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

impl<'b, R> ErrorType for Body<'b, R>
where
    R: ErrorType,
{
    type Error = Error<R::Error>;
}

impl<'b, R> Read for Body<'b, R>
where
    R: Read,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Close(read) => Ok(read.read(buf).await.map_err(Error::Io)?),
            Self::ContentLen(read) => Ok(read.read(buf).await?),
            Self::Chunked(read) => Ok(read.read(buf).await?),
        }
    }
}

pub(crate) struct PartiallyRead<'b, R> {
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

    // pub fn buf_len(&self) -> usize {
    //     self.buf.len()
    // }

    // pub fn as_raw_reader(&mut self) -> &mut R {
    //     &mut self.input
    // }

    pub fn release(self) -> R {
        self.input
    }
}

impl<'b, R> ErrorType for PartiallyRead<'b, R>
where
    R: ErrorType,
{
    type Error = R::Error;
}

impl<'b, R> Read for PartiallyRead<'b, R>
where
    R: Read,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
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

pub(crate) struct ContentLenRead<R> {
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

impl<R> ErrorType for ContentLenRead<R>
where
    R: ErrorType,
{
    type Error = Error<R::Error>;
}

impl<R> Read for ContentLenRead<R>
where
    R: Read,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
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

pub(crate) struct ChunkedRead<'b, R> {
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
            Some(s) => unsafe { str::from_utf8_unchecked(s) },
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

impl<'b, R> ErrorType for ChunkedRead<'b, R>
where
    R: ErrorType,
{
    type Error = Error<R::Error>;
}

impl<'b, R> Read for ChunkedRead<'b, R>
where
    R: Read,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
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

#[allow(private_interfaces)]
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

    pub fn needs_close(&self) -> bool {
        !self.is_complete() || matches!(self, Self::Close(_))
    }

    pub async fn finish(&mut self) -> Result<(), Error<W::Error>>
    where
        W: Write,
    {
        match self {
            Self::Close(_) => (),
            Self::ContentLen(w) => {
                if !w.is_complete() {
                    return Err(Error::IncompleteBody);
                }
            }
            Self::Chunked(w) => w.finish().await?,
        }

        self.flush().await?;

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

impl<W> ErrorType for SendBody<W>
where
    W: ErrorType,
{
    type Error = Error<W::Error>;
}

impl<W> Write for SendBody<W>
where
    W: Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Close(w) => Ok(w.write(buf).await.map_err(Error::Io)?),
            Self::ContentLen(w) => Ok(w.write(buf).await?),
            Self::Chunked(w) => Ok(w.write(buf).await?),
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Close(w) => Ok(w.flush().await.map_err(Error::Io)?),
            Self::ContentLen(w) => Ok(w.flush().await?),
            Self::Chunked(w) => Ok(w.flush().await?),
        }
    }
}

pub(crate) struct ContentLenWrite<W> {
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

impl<W> ErrorType for ContentLenWrite<W>
where
    W: ErrorType,
{
    type Error = Error<W::Error>;
}

impl<W> Write for ContentLenWrite<W>
where
    W: Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if self.content_len >= self.write_len + buf.len() as u64 {
            let write = self.output.write(buf).await.map_err(Error::Io)?;
            self.write_len += write as u64;

            Ok(write)
        } else {
            Err(Error::TooLongBody)
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.output.flush().await.map_err(Error::Io)
    }
}

pub(crate) struct ChunkedWrite<W> {
    output: W,
    finished: bool,
}

impl<W> ChunkedWrite<W> {
    pub const fn new(output: W) -> Self {
        Self {
            output,
            finished: false,
        }
    }

    pub async fn finish(&mut self) -> Result<(), Error<W::Error>>
    where
        W: Write,
    {
        if !self.finished {
            self.output
                .write_all(b"0\r\n\r\n")
                .await
                .map_err(Error::Io)?;
            self.finished = true;
        }

        Ok(())
    }

    pub fn release(self) -> W {
        self.output
    }
}

impl<W> ErrorType for ChunkedWrite<W>
where
    W: ErrorType,
{
    type Error = Error<W::Error>;
}

impl<W> Write for ChunkedWrite<W>
where
    W: Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if self.finished {
            Err(Error::InvalidState)
        } else if !buf.is_empty() {
            let mut len_str = heapless::String::<8>::new();
            write!(&mut len_str, "{:x}", buf.len()).unwrap();

            self.output
                .write_all(len_str.as_bytes())
                .await
                .map_err(Error::Io)?;

            self.output.write_all(b"\r\n").await.map_err(Error::Io)?;
            self.output.write_all(buf).await.map_err(Error::Io)?;
            self.output.write_all(b"\r\n").await.map_err(Error::Io)?;

            Ok(buf.len())
        } else {
            Ok(0)
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.output.flush().await.map_err(Error::Io)
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
    request: bool,
    http11: bool,
    token: Option<&str>,
    extra: Option<&str>,
    mut output: W,
) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    let mut written = false;

    if !request {
        send_version(&mut output, http11).await?;
        written = true;
    }

    if let Some(token) = token {
        if written {
            output.write_all(b" ").await.map_err(Error::Io)?;
        }

        output
            .write_all(token.as_bytes())
            .await
            .map_err(Error::Io)?;

        written = true;
    }

    if let Some(extra) = extra {
        if written {
            output.write_all(b" ").await.map_err(Error::Io)?;
        }

        output
            .write_all(extra.as_bytes())
            .await
            .map_err(Error::Io)?;

        written = true;
    }

    if request {
        if written {
            output.write_all(b" ").await.map_err(Error::Io)?;
        }

        send_version(&mut output, http11).await?;
    }

    output.write_all(b"\r\n").await.map_err(Error::Io)?;

    Ok(())
}

async fn send_version<W>(mut output: W, http11: bool) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    output
        .write_all(if http11 { b"HTTP/1.1" } else { b"HTTP/1.0" })
        .await
        .map_err(Error::Io)
}

#[cfg(test)]
mod test {
    use embedded_io_async::{ErrorType, Read};

    use super::*;

    struct SliceRead<'a>(&'a [u8]);

    impl<'a> ErrorType for SliceRead<'a> {
        type Error = core::convert::Infallible;
    }

    impl<'a> Read for SliceRead<'a> {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let len = core::cmp::min(buf.len(), self.0.len());
            buf[..len].copy_from_slice(&self.0[..len]);

            self.0 = &self.0[len..];

            Ok(len)
        }
    }

    #[test]
    fn test_chunked_bytes() {
        // Normal
        expect(b"A\r\nabcdefghij\r\n2\r\n42\r\n", Some(b"abcdefghij42"));
        expect(b"a\r\nabc\r\nfghij\r\n2\r\n42\r\n", Some(b"abc\r\nfghij42"));

        // Trailing headers
        expect(b"4\r\nabcd\r\n0\r\n\r\n", Some(b"abcd"));
        expect(b"4\r\nabcd\r\n0\r\nA: B\r\n\r\n", Some(b"abcd"));

        // Empty
        expect(b"", Some(b""));
        expect(b"0\r\n\r\n", Some(b""));

        // Erroneous
        expect(b"h\r\n", None);
        expect(b"\r\na", None);
        expect(b"4\r\nabcdefg", None);
    }

    fn expect(input: &[u8], expected: Option<&[u8]>) {
        embassy_futures::block_on(async move {
            let mut buf1 = [0; 64];
            let mut buf2 = [0; 64];

            let stream = SliceRead(input);
            let mut r = ChunkedRead::new(stream, &mut buf1, 0);

            if let Some(expected) = expected {
                assert!(r.read_exact(&mut buf2[..expected.len()]).await.is_ok());

                assert_eq!(&buf2[..expected.len()], expected);

                let len = r.read(&mut buf2).await;
                assert!(len.is_ok());

                assert_eq!(len.unwrap(), 0);
            } else {
                assert!(r.read(&mut buf2).await.is_err());
            }
        })
    }
}
