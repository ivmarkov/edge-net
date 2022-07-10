use core::{fmt::Display, str};

use embedded_io::asynch::Read;

use httparse::Status;

use log::trace;

use super::*;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[allow(clippy::needless_lifetimes)]
pub async fn receive<'b, const N: usize, R>(
    buf: &'b mut [u8],
    mut input: R,
) -> Result<(Request<'b, N>, Body<'b, super::PartiallyRead<'b, R>>), (R, Error<R::Error>)>
where
    R: Read,
{
    let (read_len, headers_len) = match receive_headers::<N, _>(&mut input, buf, true).await {
        Ok(read_len) => read_len,
        Err(e) => return Err((input, e)),
    };

    let mut request = Request {
        version: None,
        method: None,
        path: None,
        headers: Headers::new(),
    };

    let mut parser = httparse::Request::new(&mut request.headers.0);

    let (headers_buf, body_buf) = buf.split_at_mut(headers_len);

    let status = match parser.parse(headers_buf) {
        Ok(status) => status,
        Err(e) => return Err((input, e.into())),
    };

    if let Status::Complete(headers_len2) = status {
        if headers_len != headers_len2 {
            panic!("Should not happen. HTTP header parsing is indeterminate.")
        }

        request.version = parser.version;
        request.method = parser.method;
        request.path = parser.path;

        trace!("Received:\n{}", request);

        let body = super::receive_body(&request.headers, body_buf, read_len, input)?;

        Ok((request, body))
    } else {
        panic!("Secondary parse of already loaded buffer failed.")
    }
}

#[derive(Debug)]
pub struct Request<'b, const N: usize> {
    pub version: Option<u8>,
    pub method: Option<&'b str>,
    pub path: Option<&'b str>,
    pub headers: Headers<'b, N>,
}

impl<'b, const N: usize> Display for Request<'b, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(version) = self.version {
            writeln!(f, "Version {}", version)?;
        }

        if let Some(method) = self.method {
            writeln!(f, "{} {}", method, self.path.unwrap_or(""))?;
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
mod embedded_svc_compat {}
