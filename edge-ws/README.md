# edge-ws

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

Async + `no_std` + no-alloc implementation of the Websockets protocol.

For other protocols, look at the [edge-net](https://github.com/ivmarkov/edge-net) aggregator crate documentation.

## Examples

**NOTE**

To connect the Websocket client example to the Websocket server example - rather that to the public Websocket echo server, 
just run it with some argument, i.e.

```sh
./target/debug/examples/ws_client 1
```

### Websocket client

```rust
use anyhow::bail;
use edge_http::ws::NONCE_LEN;
use edge_ws::{FrameHeader, FrameType};
use embedded_nal_async::{AddrType, Dns, SocketAddr, TcpConnect};

use edge_http::io::client::Connection;

use rand::{thread_rng, RngCore};

use std_embedded_nal_async::Stack;

use log::*;

// NOTE: HTTP-only echo WS servers seem to be hard to find, this one might or might not work...
const PUBLIC_ECHO_SERVER: (&str, u16, &str) = ("websockets.chilkat.io", 80, "/wsChilkatEcho.ashx");
const OUR_ECHO_SERVER: (&str, u16, &str) = ("127.0.0.1", 8881, "/");

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack: Stack = Default::default();

    let mut buf = [0_u8; 8192];

    futures_lite::future::block_on(work(&stack, &mut buf)).unwrap();
}

async fn work<T: TcpConnect + Dns>(stack: &T, buf: &mut [u8]) -> Result<(), anyhow::Error>
where
    <T as Dns>::Error: Send + Sync + std::error::Error + 'static,
    <T as TcpConnect>::Error: Send + Sync + std::error::Error + 'static,
{
    let mut args = std::env::args();
    args.next(); // Skip the executable name

    let (fqdn, port, path) = if args.next().is_some() {
        OUR_ECHO_SERVER
    } else {
        PUBLIC_ECHO_SERVER
    };

    info!("About to open an HTTP connection to {fqdn} port {port}");

    let ip = stack.get_host_by_name(fqdn, AddrType::IPv4).await?;

    let mut conn: Connection<_> = Connection::new(buf, stack, SocketAddr::new(ip, port));

    let mut rng_source = thread_rng();

    let mut nonce = [0_u8; NONCE_LEN];
    rng_source.fill_bytes(&mut nonce);

    conn.initiate_ws_upgrade_request(Some(fqdn), Some("foo.com"), path, None, &nonce)
        .await?;
    conn.initiate_response().await?;

    if !conn.is_ws_upgrade_accepted(&nonce)? {
        bail!("WS upgrade failed");
    }

    conn.complete().await?;

    // Now we have the TCP socket in a state where it can be operated as a WS connection
    // Send some traffic to a WS echo server and read it back

    let (mut socket, buf) = conn.release();

    info!("Connection upgraded to WS, starting traffic now");

    for payload in ["Hello world!", "How are you?", "I'm fine, thanks!"] {
        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: payload.as_bytes().len() as _,
            mask_key: rng_source.next_u32().into(),
        };

        info!("Sending {header}, with payload \"{payload}\"");
        header.send(&mut socket).await?;
        header.send_payload(&mut socket, payload.as_bytes()).await?;

        let header = FrameHeader::recv(&mut socket).await?;
        let payload = header.recv_payload(&mut socket, buf).await?;

        match header.frame_type {
            FrameType::Text(_) => {
                info!(
                    "Got {header}, with payload \"{}\"",
                    core::str::from_utf8(payload).unwrap()
                );
            }
            FrameType::Binary(_) => {
                info!("Got {header}, with payload {payload:?}");
            }
            _ => {
                bail!("Unexpected {}", header);
            }
        }

        if !header.frame_type.is_final() {
            bail!("Unexpected fragmented frame");
        }
    }

    Ok(())
}
```

### Websocket echo server

```rust
use edge_http::io::server::{Connection, Handler, Server, ServerBuffers};
use edge_http::Method;

use edge_std_nal_async::StdTcpListen;
use edge_ws::{FrameHeader, FrameType};
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

    let acceptor = StdTcpListen::new().listen(addr.parse().unwrap()).await?;

    let mut server: Server<_, _> = Server::new(acceptor, WsHandler);

    server.process::<2, P, B>(buffers).await?;

    Ok(())
}

struct WsHandler;

impl<'b, T, const N: usize> Handler<'b, T, N> for WsHandler
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
        } else if !conn.is_ws_upgrade_request()? {
            conn.initiate_response(200, Some("OK"), &[("Content-Type", "text/plain")])
                .await?;

            conn.write_all(b"Initiate WS Upgrade request to switch this connection to WS")
                .await?;
        } else {
            conn.initiate_ws_upgrade_response().await?;

            conn.complete().await?;

            info!("Connection upgraded to WS, starting a simple WS echo server now");

            // Now we have the TCP socket in a state where it can be operated as a WS connection
            // Run a simple WS echo server here

            let mut socket = conn.raw_connection()?;

            let mut buf = [0_u8; 8192];

            loop {
                let mut header = FrameHeader::recv(&mut socket).await?;
                let payload = header.recv_payload(&mut socket, &mut buf).await?;

                match header.frame_type {
                    FrameType::Text(_) => {
                        info!(
                            "Got {header}, with payload \"{}\"",
                            core::str::from_utf8(payload).unwrap()
                        );
                    }
                    FrameType::Binary(_) => {
                        info!("Got {header}, with payload {payload:?}");
                    }
                    FrameType::Close => {
                        info!("Got {header}");
                        break;
                    }
                    _ => {
                        info!("Got {header}");
                    }
                }

                // Echo it back now

                header.mask_key = None; // Servers never mask the payload

                if matches!(header.frame_type, FrameType::Ping) {
                    header.frame_type = FrameType::Pong;
                }

                info!("Echoing back as {header}");

                header.send(&mut socket).await?;
                header.send_payload(&mut socket, payload).await?;
            }
        }

        Ok(())
    }
}
```
