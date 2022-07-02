use core::{cmp::max, str};

use uncased::UncasedStr;

pub mod client;

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
    fn as_str(&self) -> &str {
        "TODO" // TODO
    }
}

struct HttpHeadersSplitter<'a>(&'a [u8], usize);

impl<'a> HttpHeadersSplitter<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self(buf, 0)
    }
}

impl<'a> Iterator for HttpHeadersSplitter<'a> {
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

pub(crate) struct HttpSendHeaders<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> HttpSendHeaders<'a> {
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
            self.append(b"\r\n");
        }
    }

    pub(crate) fn buf(&self) -> &[u8] {
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
        for (start, end) in HttpHeadersSplitter::new(&self.buf[..self.len]).skip(1) {
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
        HttpHeadersSplitter::new(&self.buf[..self.len])
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
