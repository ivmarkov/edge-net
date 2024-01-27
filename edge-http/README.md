# edge-http

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

Async + `no_std` + no-alloc implementation of the HTTP protocol.

The implementation is based on the splendid [httparse](https://github.com/seanmonstar/httparse) library.

For other protocols, look at the [edge-net](https://github.com/ivmarkov/edge-net) aggregator crate documentation.

## Examples

### HTTP client

```rust
use embedded_io_async::Read;
use embedded_nal_async::{AddrType, Dns, SocketAddr, TcpConnect};

use edge_http::io::{client::Connection, Error};
use edge_http::Method;

use log::*;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack: edge_std_nal_async::Stack = Default::default();

    let mut buf = [0_u8; 8192];

    futures_lite::future::block_on(read(&stack, &mut buf)).unwrap();
}

async fn read<T: TcpConnect + Dns>(
    stack: &T,
    buf: &mut [u8],
) -> Result<(), Error<<T as TcpConnect>::Error>>
where
    <T as Dns>::Error: Into<<T as TcpConnect>::Error>,
{
    info!("About to open an HTTP connection to httpbin.org port 80");

    let ip = stack
        .get_host_by_name("httpbin.org", AddrType::IPv4)
        .await
        .map_err(|e| Error::Io(e.into()))?;

    let mut conn: Connection<_> = Connection::new(buf, stack, SocketAddr::new(ip, 80));

    for uri in ["/ip", "/headers"] {
        request(&mut conn, uri).await?;
    }

    Ok(())
}

async fn request<'b, const N: usize, T: TcpConnect>(
    conn: &mut Connection<'b, T, N>,
    uri: &str,
) -> Result<(), Error<T::Error>> {
    conn.initiate_request(true, Method::Get, uri, &[("Host", "httpbin.org")])
        .await?;

    conn.initiate_response().await?;

    let mut result = Vec::new();

    let mut buf = [0_u8; 1024];

    loop {
        let len = conn.read(&mut buf).await?;

        if len > 0 {
            result.extend_from_slice(&buf[0..len]);
        } else {
            break;
        }
    }

    info!(
        "Request to httpbin.org, URI \"{}\" returned:\nBody:\n=================\n{}\n=================\n\n\n\n",
        uri,
        core::str::from_utf8(&result).unwrap());

    Ok(())
}
```

### HTTP server

```rust
use edge_http::io::server::{Connection, Handler, Server, ServerBuffers};
use edge_http::Method;

use embedded_nal_async_xtra::TcpListen;

use embedded_io_async::{Read, Write};

use log::info;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let mut buffers: ServerBuffers = ServerBuffers::new();

    futures_lite::future::block_on(run(&mut buffers)).unwrap();
}

pub async fn run<const P: usize, const B: usize>(
    buffers: &mut ServerBuffers<P, B>,
) -> Result<(), anyhow::Error> {
    let addr = "0.0.0.0:8881";

    info!("Running HTTP server on {addr}");

    let acceptor = edge_std_nal_async::Stack::new()
        .listen(addr.parse().unwrap())
        .await?;

    let mut server: Server<_, _> = Server::new(acceptor, HttpHandler);

    server.process::<2, P, B>(buffers).await?;

    Ok(())
}

struct HttpHandler;

impl<'b, T, const N: usize> Handler<'b, T, N> for HttpHandler
where
    T: Read + Write,
    T::Error: Send + Sync + std::error::Error + 'static,
{
    type Error = anyhow::Error;

    async fn handle(&self, conn: &mut Connection<'b, T, N>) -> Result<(), Self::Error> {
        let headers = conn.headers()?;

        if !matches!(headers.method, Some(Method::Get)) {
            conn.initiate_response(405, Some("Method Not Allowed"), &[])
                .await?;
        } else if !matches!(headers.path, Some("/")) {
            conn.initiate_response(404, Some("Not Found"), &[]).await?;
        } else {
            conn.initiate_response(200, Some("OK"), &[("Content-Type", "text/plain")])
                .await?;

            conn.write_all(b"Hello world!").await?;
        }

        Ok(())
    }
}
```
