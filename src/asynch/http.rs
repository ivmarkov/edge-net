use core::cmp::{max, min};
use core::future::Future;
use core::str;

use embedded_io::asynch::Read;
use embedded_io::Io;

use uncased::UncasedStr;

pub mod client;
pub mod server;

/// An error in parsing.
#[derive(Debug)]
pub enum Error<R> {
    /// Invalid byte in header name.
    HeaderName,
    /// Invalid byte in header value.
    HeaderValue,
    /// Invalid byte in new line.
    NewLine,
    /// Invalid byte in Response status.
    Status,
    /// Invalid byte where token is required.
    Token,
    /// Parsed more headers than provided buffer can contain.
    TooManyHeaders,
    /// Invalid byte in HTTP version.
    Version,
    /// Incomplete request/response.
    Incomplete,
    /// Read error.
    Read(R),
}

impl<R> From<httparse::Error> for Error<R> {
    fn from(e: httparse::Error) -> Self {
        match e {
            httparse::Error::HeaderName => Self::HeaderName,
            httparse::Error::HeaderValue => Self::HeaderValue,
            httparse::Error::NewLine => Self::NewLine,
            httparse::Error::Status => Self::Status,
            httparse::Error::Token => Self::Token,
            httparse::Error::TooManyHeaders => Self::TooManyHeaders,
            httparse::Error::Version => Self::Version,
        }
    }
}

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
    fn new(method: &str) -> Option<Self> {
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

pub struct Headers<'b, const N: usize>([httparse::Header<'b>; N]);

impl<'b, const N: usize> Headers<'b, N> {
    pub fn new() -> Self {
        Self([httparse::EMPTY_HEADER; N])
    }
}

pub struct Body<'b, R> {
    buf: &'b [u8],
    content_len: usize,
    read_len: usize,
    input: R,
}

impl<'b, R> Io for Body<'b, R>
where
    R: Io,
{
    type Error = R::Error;
}

impl<'b, R> Read for Body<'b, R>
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

        for (index, ch) in slice.iter().enumerate() {
            if *ch == b'\r' && index < slice.len() - 1 && slice[index + 1] == b'\n' {
                let result = (self.1, self.1 + index + 2);

                self.1 = result.1;

                return Some(result);
            }
        }

        None
    }
}

pub(crate) struct SendHeaders<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> SendHeaders<'a> {
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    pub(crate) fn set_status_tokens(&mut self, tokens: &[&str]) {
        if let Some(old_end) = self.get_status_len() {
            let new_end = tokens
                .iter()
                .map(|token| token.as_bytes().len())
                .max()
                .unwrap_or(0)
                + max(tokens.len(), 1)
                - 1;

            self.shift(old_end, new_end);

            let mut offset = 0;
            for (index, token) in tokens.iter().enumerate() {
                let bytes = token.as_bytes();

                self.buf[offset..bytes.len()].copy_from_slice(bytes);
                offset += bytes.len();

                if index < tokens.len() {
                    self.buf[offset] = b' ';
                    offset += 1;
                }
            }
        } else {
            for (index, token) in tokens.iter().enumerate() {
                self.append(token.as_bytes());

                if index < tokens.len() {
                    self.append(b" ");
                }
            }

            self.append(b"\r\n");
        }
    }

    pub(crate) fn set(&mut self, name: &str, value: &str) {
        self.set_raw(name, value.as_bytes());
    }

    pub(crate) fn set_raw(&mut self, name: &str, value: &[u8]) {
        if let Some((start, end)) = self.get_loc(name) {
            self.set_at(value, start, end);
        } else {
            self.append(name.as_bytes());
            self.append(b":");
            self.append(value);
            self.append(b"\r\n");
        }
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn set_at(&mut self, value: &[u8], start: usize, end: usize) {
        self.shift(end, start + value.len());
        self.buf[start..start + value.len()].copy_from_slice(value);
    }

    fn append(&mut self, value: &[u8]) {
        self.buf[self.len..self.len + value.len()].copy_from_slice(value);
        self.len += value.len();
    }

    fn shift(&mut self, old_offset: usize, new_offset: usize) {
        if new_offset > old_offset {
            let delta = new_offset - old_offset;

            for index in (new_offset..self.len + delta).rev() {
                self.buf[index] = self.buf[index - delta];
            }

            self.len += delta;
        } else if new_offset < old_offset {
            let delta = old_offset - new_offset;

            for index in new_offset..self.len - delta {
                self.buf[index] = self.buf[index + delta];
            }

            self.len -= delta;
        }
    }

    fn get_loc(&self, name: &str) -> Option<(usize, usize)> {
        for (start, end) in SendHeadersSplitter::new(&self.buf[..self.len]).skip(1) {
            let value_start = self.get_header_value_start(start, end).unwrap();

            if UncasedStr::new(name)
                == UncasedStr::new(unsafe {
                    str::from_utf8_unchecked(&self.buf[start..value_start - 1])
                })
            {
                return Some((value_start, end - 2));
            }
        }

        None
    }

    fn get_status_len(&self) -> Option<usize> {
        SendHeadersSplitter::new(&self.buf[..self.len])
            .next()
            .map(|(_, end)| end)
    }

    fn get_header_value_start(&self, start: usize, end: usize) -> Option<usize> {
        let slice = &self.buf[start..end];

        for (index, ch) in slice.iter().enumerate() {
            if *ch == b':' {
                return Some(index + 1);
            }
        }

        None
    }
}
