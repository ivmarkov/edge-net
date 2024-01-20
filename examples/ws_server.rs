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
