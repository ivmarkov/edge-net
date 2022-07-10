use core::{fmt::Display, str};

use embedded_io::asynch::Read;

use httparse::Status;

use uncased::UncasedStr;

use log::trace;

use super::*;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

pub fn get<'b>(uri: &str, buf: &'b mut [u8]) -> SendHeaders<'b> {
    request(Method::Get, uri, buf)
}

pub fn post<'b>(uri: &str, buf: &'b mut [u8]) -> SendHeaders<'b> {
    request(Method::Post, uri, buf)
}

pub fn put<'b>(uri: &str, buf: &'b mut [u8]) -> SendHeaders<'b> {
    request(Method::Put, uri, buf)
}

pub fn delete<'b>(uri: &str, buf: &'b mut [u8]) -> SendHeaders<'b> {
    request(Method::Delete, uri, buf)
}

pub fn request<'b>(method: Method, uri: &str, buf: &'b mut [u8]) -> SendHeaders<'b> {
    SendHeaders::new(buf, &[method.as_str(), uri, "HTTP/1.1"])
}

#[allow(clippy::needless_lifetimes)]
pub async fn send<'b, W>(
    headers: SendHeaders<'b>,
    mut output: W,
) -> Result<(&'b mut [u8], SendBody<W>), (W, W::Error)>
where
    W: Write,
{
    trace!("Sending request:\n{}", headers);

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

#[allow(clippy::needless_lifetimes)]
pub async fn receive<'b, const N: usize, R>(
    buf: &'b mut [u8],
    mut input: R,
) -> Result<(Response<'b, N>, Body<'b, super::PartiallyRead<'b, R>>), (R, Error<R::Error>)>
where
    R: Read,
{
    let (read_len, response_len) = match receive_headers::<N, _>(&mut input, buf).await {
        Ok(read_len) => read_len,
        Err(e) => return Err((input, e)),
    };

    let mut response = Response {
        version: None,
        code: None,
        reason: None,
        headers: Headers::new(),
    };

    let mut http_response = httparse::Response::new(&mut response.headers.0);

    let (response_buf, body_buf) = buf.split_at_mut(response_len);

    let status = match http_response.parse(response_buf) {
        Ok(status) => status,
        Err(e) => return Err((input, e.into())),
    };

    if let Status::Complete(response_len2) = status {
        if response_len != response_len2 {
            panic!("Should not happen. HTTP header parsing is indeterminate.")
        }

        response.version = http_response.version;
        response.code = http_response.code;
        response.reason = http_response.reason;

        trace!("Got response:\n{}", response);

        let body = if response
            .headers
            .transfer_encoding()
            .map(|value| UncasedStr::new(value) == UncasedStr::new("chunked"))
            .unwrap_or(false)
        {
            Body::Chunked(ChunkedRead::new(
                PartiallyRead::new(&[], input),
                body_buf,
                read_len - response_len,
            ))
        } else if let Some(content_len) = response.headers.content_len() {
            Body::ContentLen(ContentLenRead::new(
                content_len,
                PartiallyRead::new(body_buf, input),
            ))
        } else if response
            .headers
            .connection()
            .map(|value| UncasedStr::new(value) == UncasedStr::new("close"))
            .unwrap_or(false)
        {
            Body::Close(PartiallyRead::new(body_buf, input))
        } else {
            Body::ContentLen(ContentLenRead::new(0, PartiallyRead::new(body_buf, input)))
        };

        Ok((response, body))
    } else {
        panic!("Secondary parse of already loaded buffer failed.")
    }
}

#[derive(Debug)]
pub struct Response<'b, const N: usize> {
    pub version: Option<u8>,
    pub code: Option<u16>,
    pub reason: Option<&'b str>,
    pub headers: Headers<'b, N>,
}

impl<'b, const N: usize> Display for Response<'b, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(version) = self.version {
            writeln!(f, "Version {}", version)?;
        }

        if let Some(code) = self.code {
            writeln!(f, "{} {}", code, self.reason.unwrap_or(""))?;
        }

        for (name, value) in self.headers.headers() {
            if name.is_empty() {
                break;
            }

            writeln!(f, "{}: {}", name, value)?;
        }

        Ok(())
    }
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;
    use core::str;

    use log::info;
    use no_std_net::SocketAddr;

    use embedded_svc::http::client::asynch::Method;
    use embedded_svc::io::asynch::{Io, Read, Write};

    use crate::asynch::http::{Error, PartiallyRead, SendHeaders};
    use crate::asynch::tcp::TcpClientSocket;
    use crate::close::Close;

    pub struct Client<'b, const N: usize, T>
    where
        T: TcpClientSocket + 'b,
    {
        buf: &'b mut [u8],
        socket: T,
        addr: SocketAddr,
    }

    impl<'b, const N: usize, T> Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        pub fn new(buf: &'b mut [u8], socket: T, addr: SocketAddr) -> Self {
            Self { buf, socket, addr }
        }
    }

    impl<'b, const N: usize, T> embedded_svc::io::asynch::Io for Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Client for Client<'b, N, T>
    where
        T: TcpClientSocket + 'b,
    {
        type Request<'a>
        where
            Self: 'a,
        = ClientRequest<'a, N, Completion<&'a mut T>>;

        type RequestFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<Self::Request<'a>, Self::Error>>;

        fn request<'a>(&'a mut self, method: Method, uri: &'a str) -> Self::RequestFuture<'a> {
            async move {
                if !self.socket.is_connected().await.map_err(Error::Io)? {
                    // TODO: Need to validate that the socket is still alive

                    self.socket.connect(self.addr).await.map_err(Error::Io)?;
                }

                Ok(Self::Request::new(
                    method,
                    uri,
                    self.buf,
                    Completion::new(&mut self.socket),
                ))
            }
        }
    }

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

    impl<'b> embedded_svc::http::client::asynch::SendHeaders for SendHeaders<'b> {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.header(name, value)
        }
    }

    pub struct ClientRequest<'b, const N: usize, T>
    where
        T: Close,
    {
        headers: SendHeaders<'b>,
        io: Completion<T>,
    }

    impl<'b, const N: usize, T> ClientRequest<'b, N, T>
    where
        T: Read + Write + Close,
    {
        pub fn new(
            method: embedded_svc::http::client::asynch::Method,
            uri: &str,
            buf: &'b mut [u8],
            io: T,
        ) -> Self {
            Self {
                headers: super::request(method.into(), uri, buf),
                io: Completion::new(io),
            }
        }
    }

    impl<'b, const N: usize, T> Io for ClientRequest<'b, N, T>
    where
        T: Io + Close,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::SendHeaders
        for ClientRequest<'b, N, T>
    where
        T: Close,
    {
        fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
            self.headers.set_header(name, value);
            self
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::Request for ClientRequest<'b, N, T>
    where
        T: Read + Write + Close,
    {
        type Write = ClientRequestWrite<'b, N, SendBodyCompletionTracker<T>>;

        type IntoWriterFuture = impl Future<
            Output = Result<ClientRequestWrite<'b, N, SendBodyCompletionTracker<T>>, Self::Error>,
        >;

        type SubmitFuture = impl Future<
            Output = Result<ClientResponse<'b, N, BodyCompletionTracker<'b, T>>, Self::Error>,
        >;

        fn into_writer(self) -> Self::IntoWriterFuture
        where
            Self: Sized,
        {
            async move {
                match super::send(self.headers, self.io).await {
                    Ok((buf, mut body)) => {
                        let complete = body.is_complete();

                        body.as_raw_writer().write_complete(complete);

                        Ok(ClientRequestWrite {
                            buf,
                            io: SendBodyCompletionTracker(body),
                        })
                    }
                    Err((mut io, e)) => {
                        io.close();

                        Err(Error::Io(e))
                    }
                }
            }
        }

        fn submit(self) -> Self::SubmitFuture
        where
            Self: Sized,
        {
            use embedded_svc::http::client::asynch::RequestWrite;

            async move { self.into_writer().await?.into_response().await }
        }
    }

    pub struct ClientRequestWrite<'b, const N: usize, T> {
        buf: &'b mut [u8],
        io: T,
    }

    impl<'b, const N: usize, T> Io for ClientRequestWrite<'b, N, T>
    where
        T: Io,
    {
        type Error = T::Error;
    }

    impl<'b, const N: usize, T> Write for ClientRequestWrite<'b, N, T>
    where
        T: Write,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = T::WriteFuture<'a>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            self.io.write(buf)
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = T::FlushFuture<'a>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            self.io.flush()
        }
    }

    impl<'b, const N: usize, T> embedded_svc::http::client::asynch::RequestWrite
        for ClientRequestWrite<'b, N, SendBodyCompletionTracker<T>>
    where
        T: Read + Write + Close,
    {
        type Response = ClientResponse<'b, N, BodyCompletionTracker<'b, T>>;

        type IntoResponseFuture = impl Future<Output = Result<Self::Response, Self::Error>>;

        fn into_response(mut self) -> Self::IntoResponseFuture
        where
            Self: Sized,
        {
            async move {
                self.io.flush().await?;

                let mut body = self.io.0;

                if !body.is_complete() {
                    body.close();

                    Err(Error::IncompleteBody)
                } else {
                    match super::receive(self.buf, body.release()).await {
                        Ok((response, mut body)) => {
                            let read_complete = body.is_complete();
                            body.as_raw_reader()
                                .as_raw_reader()
                                .read_complete(read_complete);

                            Ok(Self::Response {
                                response,
                                io: BodyCompletionTracker(body),
                            })
                        }
                        Err((mut io, e)) => {
                            io.close();

                            Err(e)
                        }
                    }
                }
            }
        }
    }

    impl<'b, const N: usize> embedded_svc::http::client::asynch::Status for super::Response<'b, N> {
        fn status(&self) -> u16 {
            self.code.unwrap_or(200)
        }

        fn status_message(&self) -> Option<&'_ str> {
            self.reason
        }
    }

    impl<'b, const N: usize> embedded_svc::http::client::asynch::Headers for super::Response<'b, N> {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.headers.header(name)
        }
    }

    pub struct ClientResponse<'b, const N: usize, R> {
        response: super::Response<'b, N>,
        io: R,
    }

    impl<'b, const N: usize, R> embedded_svc::http::client::asynch::Status
        for ClientResponse<'b, N, R>
    {
        fn status(&self) -> u16 {
            self.response.code.unwrap_or(200)
        }

        fn status_message(&self) -> Option<&'_ str> {
            self.response.reason
        }
    }

    impl<'b, const N: usize, R> embedded_svc::http::client::asynch::Headers
        for ClientResponse<'b, N, R>
    {
        fn header(&self, name: &str) -> Option<&'_ str> {
            self.response.header(name)
        }
    }

    impl<'b, const N: usize, R> embedded_svc::io::Io for ClientResponse<'b, N, R>
    where
        R: Io,
    {
        type Error = R::Error;
    }

    impl<'b, const N: usize, R> embedded_svc::io::asynch::Read for ClientResponse<'b, N, R>
    where
        R: Read,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = R::ReadFuture<'a>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            self.io.read(buf)
        }
    }

    impl<'b, const N: usize, R> embedded_svc::http::client::asynch::Response
        for ClientResponse<'b, N, R>
    where
        R: Read,
    {
        type Headers = super::Response<'b, N>;

        type Body = R;

        fn split(self) -> (Self::Headers, Self::Body)
        where
            Self: Sized,
        {
            (self.response, self.io)
        }
    }

    pub struct Completion<T>
    where
        T: Close,
    {
        io: T,
        read_complete: bool,
        write_complete: bool,
    }

    impl<T> Completion<T>
    where
        T: Close,
    {
        const fn new(io: T) -> Self {
            Self {
                io,
                read_complete: true,
                write_complete: true,
            }
        }

        fn read_complete(&mut self, complete: bool) {
            self.read_complete = complete;
        }

        fn write_complete(&mut self, complete: bool) {
            self.write_complete = complete;
        }
    }

    impl<T> Drop for Completion<T>
    where
        T: Close,
    {
        fn drop(&mut self) {
            if !self.read_complete || !self.write_complete {
                self.close();
            }
        }
    }

    impl<T> Close for Completion<T>
    where
        T: Close,
    {
        fn close(&mut self) {
            info!("Socket closed");

            self.io.close();
        }
    }

    impl<T> Io for Completion<T>
    where
        T: Io + Close,
    {
        type Error = T::Error;
    }

    impl<T> Read for Completion<T>
    where
        T: Read + Close,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = T::ReadFuture<'a>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            self.io.read(buf)
        }
    }

    impl<T> Write for Completion<T>
    where
        T: Write + Close,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = T::WriteFuture<'a>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            self.io.write(buf)
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = T::FlushFuture<'a>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            self.io.flush()
        }
    }

    pub struct BodyCompletionTracker<'b, T>(super::Body<'b, PartiallyRead<'b, Completion<T>>>)
    where
        T: Close;

    impl<'b, T> Io for BodyCompletionTracker<'b, T>
    where
        T: Io + Close,
    {
        type Error = super::Error<T::Error>;
    }

    impl<'b, T> Read for BodyCompletionTracker<'b, T>
    where
        T: Read + Close,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move {
                let size = self.0.read(buf).await.map_err(|e| {
                    self.0.close();
                    e
                })?;

                let complete = self.0.is_complete();
                self.0
                    .as_raw_reader()
                    .as_raw_reader()
                    .read_complete(complete);

                Ok(size)
            }
        }
    }

    pub struct SendBodyCompletionTracker<T>(super::SendBody<Completion<T>>)
    where
        T: Close;

    impl<T> Io for SendBodyCompletionTracker<T>
    where
        T: Io + Close,
    {
        type Error = super::Error<T::Error>;
    }

    impl<T> Write for SendBodyCompletionTracker<T>
    where
        T: Write + Close,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            async move {
                let size = self.0.write(buf).await.map_err(|e| {
                    self.0.close();
                    e
                })?;

                let complete = self.0.is_complete();
                self.0.as_raw_writer().write_complete(complete);

                Ok(size)
            }
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            async move {
                self.0.flush().await.map_err(|e| {
                    self.0.close();
                    e
                })
            }
        }
    }
}
