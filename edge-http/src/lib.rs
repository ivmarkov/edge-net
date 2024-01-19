#![cfg_attr(not(feature = "std"), no_std)]
#![allow(async_fn_in_trait)]

use core::fmt::Display;
use core::str;

use httparse::{Header, EMPTY_HEADER};

#[cfg(feature = "io")]
pub mod io;

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
        if method.eq_ignore_ascii_case("Delete") {
            Some(Self::Delete)
        } else if method.eq_ignore_ascii_case("Get") {
            Some(Self::Get)
        } else if method.eq_ignore_ascii_case("Head") {
            Some(Self::Head)
        } else if method.eq_ignore_ascii_case("Post") {
            Some(Self::Post)
        } else if method.eq_ignore_ascii_case("Put") {
            Some(Self::Put)
        } else if method.eq_ignore_ascii_case("Connect") {
            Some(Self::Connect)
        } else if method.eq_ignore_ascii_case("Options") {
            Some(Self::Options)
        } else if method.eq_ignore_ascii_case("Trace") {
            Some(Self::Trace)
        } else if method.eq_ignore_ascii_case("Copy") {
            Some(Self::Copy)
        } else if method.eq_ignore_ascii_case("Lock") {
            Some(Self::Lock)
        } else if method.eq_ignore_ascii_case("MkCol") {
            Some(Self::MkCol)
        } else if method.eq_ignore_ascii_case("Move") {
            Some(Self::Move)
        } else if method.eq_ignore_ascii_case("Propfind") {
            Some(Self::Propfind)
        } else if method.eq_ignore_ascii_case("Proppatch") {
            Some(Self::Proppatch)
        } else if method.eq_ignore_ascii_case("Search") {
            Some(Self::Search)
        } else if method.eq_ignore_ascii_case("Unlock") {
            Some(Self::Unlock)
        } else if method.eq_ignore_ascii_case("Bind") {
            Some(Self::Bind)
        } else if method.eq_ignore_ascii_case("Rebind") {
            Some(Self::Rebind)
        } else if method.eq_ignore_ascii_case("Unbind") {
            Some(Self::Unbind)
        } else if method.eq_ignore_ascii_case("Acl") {
            Some(Self::Acl)
        } else if method.eq_ignore_ascii_case("Report") {
            Some(Self::Report)
        } else if method.eq_ignore_ascii_case("MkActivity") {
            Some(Self::MkActivity)
        } else if method.eq_ignore_ascii_case("Checkout") {
            Some(Self::Checkout)
        } else if method.eq_ignore_ascii_case("Merge") {
            Some(Self::Merge)
        } else if method.eq_ignore_ascii_case("MSearch") {
            Some(Self::MSearch)
        } else if method.eq_ignore_ascii_case("Notify") {
            Some(Self::Notify)
        } else if method.eq_ignore_ascii_case("Subscribe") {
            Some(Self::Subscribe)
        } else if method.eq_ignore_ascii_case("Unsubscribe") {
            Some(Self::Unsubscribe)
        } else if method.eq_ignore_ascii_case("Patch") {
            Some(Self::Patch)
        } else if method.eq_ignore_ascii_case("Purge") {
            Some(Self::Purge)
        } else if method.eq_ignore_ascii_case("MkCalendar") {
            Some(Self::MkCalendar)
        } else if method.eq_ignore_ascii_case("Link") {
            Some(Self::Link)
        } else if method.eq_ignore_ascii_case("Unlink") {
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

#[derive(Debug)]
pub struct Headers<'b, const N: usize = 64>([httparse::Header<'b>; N]);

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

    pub fn host(&self) -> Option<&str> {
        self.get("Host")
    }

    pub fn connection(&self) -> Option<&str> {
        self.get("Connection")
    }

    pub fn cache_control(&self) -> Option<&str> {
        self.get("Cache-Control")
    }

    pub fn is_ws_upgrade_request(&self) -> bool {
        crate::ws::is_upgrade_request(self.iter())
    }

    pub fn upgrade(&self) -> Option<&str> {
        self.get("Upgrade")
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
            .find(|(hname, _)| name.eq_ignore_ascii_case(hname))
            .map(|(_, value)| value)
    }

    pub fn get_raw(&self, name: &str) -> Option<&[u8]> {
        self.iter_raw()
            .find(|(hname, _)| name.eq_ignore_ascii_case(hname))
            .map(|(_, value)| value)
    }

    pub fn set(&mut self, name: &'b str, value: &'b str) -> &mut Self {
        self.set_raw(name, value.as_bytes())
    }

    pub fn set_raw(&mut self, name: &'b str, value: &'b [u8]) -> &mut Self {
        if !name.is_empty() {
            for header in &mut self.0 {
                if header.name.is_empty() || header.name.eq_ignore_ascii_case(name) {
                    *header = Header { name, value };
                    return self;
                }
            }
        }

        panic!("No space left");
    }

    pub fn remove(&mut self, name: &str) -> &mut Self {
        let index = self
            .0
            .iter()
            .enumerate()
            .find(|(_, header)| header.name.eq_ignore_ascii_case(name));

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
        *buf = content_len.try_into().unwrap();

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

    pub fn set_host(&mut self, host: &'b str) -> &mut Self {
        self.set("Host", host)
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
        host: Option<&'b str>,
        origin: Option<&'b str>,
        version: Option<&'b str>,
        nonce: &[u8; ws::NONCE_LEN],
        nonce_base64_buf: &'b mut [u8; ws::MAX_BASE64_KEY_LEN],
    ) -> &mut Self {
        for (name, value) in
            ws::upgrade_request_headers(host, origin, version, nonce, nonce_base64_buf)
        {
            self.set(name, value);
        }

        self
    }

    pub fn set_ws_upgrade_response_headers<'a, H>(
        &mut self,
        request_headers: H,
        version: Option<&'a str>,
        sec_key_response_base64_buf: &'b mut [u8; ws::MAX_BASE64_KEY_RESPONSE_LEN],
    ) -> Result<&mut Self, ws::UpgradeError>
    where
        H: IntoIterator<Item = (&'a str, &'a str)>,
    {
        for (name, value) in
            ws::upgrade_response_headers(request_headers, version, sec_key_response_base64_buf)?
        {
            self.set(name, value);
        }

        Ok(self)
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
        if "Transfer-Encoding".eq_ignore_ascii_case(name) {
            if value.eq_ignore_ascii_case("Chunked") {
                return Self::Chunked;
            }
        } else if "Content-Length".eq_ignore_ascii_case(name) {
            return Self::ContentLen(value.parse::<u64>().unwrap()); // TODO
        } else if "Connection".eq_ignore_ascii_case(name) && value.eq_ignore_ascii_case("Close") {
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

#[derive(Default, Debug)]
pub struct RequestHeaders<'b, const N: usize> {
    pub http11: Option<bool>,
    pub method: Option<Method>,
    pub path: Option<&'b str>,
    pub headers: Headers<'b, N>,
}

impl<'b, const N: usize> RequestHeaders<'b, N> {
    pub const fn new() -> Self {
        Self {
            http11: Some(true),
            method: None,
            path: None,
            headers: Headers::<N>::new(),
        }
    }
}

impl<'b, const N: usize> Display for RequestHeaders<'b, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(http11) = self.http11 {
            write!(f, "{} ", if http11 { "HTTP/1.1" } else { "HTTP/1.0" })?;
        }

        if let Some(method) = self.method {
            writeln!(f, "{method} {}", self.path.unwrap_or(""))?;
        }

        for (name, value) in self.headers.iter() {
            if name.is_empty() {
                break;
            }

            writeln!(f, "{name}: {value}")?;
        }

        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct ResponseHeaders<'b, const N: usize> {
    pub http11: Option<bool>,
    pub code: Option<u16>,
    pub reason: Option<&'b str>,
    pub headers: Headers<'b, N>,
}

impl<'b, const N: usize> ResponseHeaders<'b, N> {
    pub const fn new() -> Self {
        Self {
            http11: Some(true),
            code: None,
            reason: None,
            headers: Headers::<N>::new(),
        }
    }
}

impl<'b, const N: usize> Display for ResponseHeaders<'b, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(http11) = self.http11 {
            writeln!(f, "{} ", if http11 { "HTTP/1.1 " } else { "HTTP/1.0" })?;
        }

        if let Some(code) = self.code {
            writeln!(f, "{code} {}", self.reason.unwrap_or(""))?;
        }

        for (name, value) in self.headers.iter() {
            if name.is_empty() {
                break;
            }

            writeln!(f, "{name}: {value}")?;
        }

        Ok(())
    }
}

pub mod ws {
    pub const NONCE_LEN: usize = 16;
    pub const MAX_BASE64_KEY_LEN: usize = 28;
    pub const MAX_BASE64_KEY_RESPONSE_LEN: usize = 33;

    pub const UPGRADE_REQUEST_HEADERS_LEN: usize = 7;
    pub const UPGRADE_RESPONSE_HEADERS_LEN: usize = 3;

    pub fn upgrade_request_headers<'a>(
        host: Option<&'a str>,
        origin: Option<&'a str>,
        version: Option<&'a str>,
        nonce: &[u8; NONCE_LEN],
        nonce_base64_buf: &'a mut [u8; MAX_BASE64_KEY_LEN],
    ) -> [(&'a str, &'a str); UPGRADE_REQUEST_HEADERS_LEN] {
        let nonce_base64_len =
            base64::encode_config_slice(nonce, base64::URL_SAFE, nonce_base64_buf);

        let host = host.map(|host| ("Host", host)).unwrap_or(("", ""));
        let origin = origin.map(|origin| ("Origin", origin)).unwrap_or(("", ""));

        [
            host,
            origin,
            ("Content-Length", "0"),
            ("Connection", "Upgrade"),
            ("Upgrade", "websocket"),
            ("Sec-WebSocket-Version", version.unwrap_or("13")),
            ("Sec-WebSocket-Key", unsafe {
                core::str::from_utf8_unchecked(&nonce_base64_buf[..nonce_base64_len])
            }),
        ]
    }

    pub fn is_upgrade_request<'a, H>(request_headers: H) -> bool
    where
        H: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut connection = false;
        let mut upgrade = false;

        for (name, value) in request_headers {
            if name.eq_ignore_ascii_case("Connection") {
                connection = value.eq_ignore_ascii_case("Upgrade");
            } else if name.eq_ignore_ascii_case("Upgrade") {
                upgrade = value.eq_ignore_ascii_case("websocket");
            }
        }

        connection && upgrade
    }

    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    pub enum UpgradeError {
        NoVersion,
        NoSecKey,
        UnsupportedVersion,
        SecKeyTooLong,
    }

    pub fn upgrade_response_headers<'a, 'b, H>(
        request_headers: H,
        version: Option<&'a str>,
        sec_key_response_base64_buf: &'b mut [u8; MAX_BASE64_KEY_RESPONSE_LEN],
    ) -> Result<[(&'b str, &'b str); UPGRADE_RESPONSE_HEADERS_LEN], UpgradeError>
    where
        H: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut version_ok = false;
        let mut sec_key = None;

        for (name, value) in request_headers {
            if name.eq_ignore_ascii_case("Sec-WebSocket-Version") {
                if !value.eq_ignore_ascii_case(version.unwrap_or("13")) {
                    return Err(UpgradeError::NoVersion);
                }

                version_ok = true;
            } else if name.eq_ignore_ascii_case("Sec-WebSocket-Key") {
                const WS_MAGIC_GUUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

                let mut buf = [0_u8; MAX_BASE64_KEY_LEN + WS_MAGIC_GUUID.len()];

                let value_len = value.as_bytes().len();

                if value_len > MAX_BASE64_KEY_LEN {
                    return Err(UpgradeError::SecKeyTooLong);
                }

                buf[..value_len].copy_from_slice(value.as_bytes());
                buf[value_len..value_len + WS_MAGIC_GUUID.as_bytes().len()]
                    .copy_from_slice(WS_MAGIC_GUUID.as_bytes());

                let mut sha1 = sha1_smol::Sha1::new();

                sha1.update(&buf[..value_len + WS_MAGIC_GUUID.as_bytes().len()]);

                let sec_key_len = base64::encode_config_slice(
                    sha1.digest().bytes(),
                    base64::STANDARD_NO_PAD,
                    sec_key_response_base64_buf,
                );

                sec_key = Some(sec_key_len);
            }
        }

        if version_ok {
            if let Some(sec_key_len) = sec_key {
                Ok([
                    ("Connection", "Upgrade"),
                    ("Upgrade", "websocket"),
                    ("Sec-WebSocket-Accept", unsafe {
                        core::str::from_utf8_unchecked(&sec_key_response_base64_buf[..sec_key_len])
                    }),
                ])
            } else {
                Err(UpgradeError::NoSecKey)
            }
        } else {
            Err(UpgradeError::NoVersion)
        }
    }
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
