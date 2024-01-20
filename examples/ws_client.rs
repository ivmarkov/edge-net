use anyhow::bail;
use edge_http::ws::NONCE_LEN;
use edge_ws::{FrameHeader, FrameType};
use embedded_nal_async::{AddrType, Dns, SocketAddr, TcpConnect};

use edge_http::io::client::Connection;

use rand::{thread_rng, RngCore};

use std_embedded_nal_async::Stack;

use log::*;

// NOTE: HTTP-only echo WS servers seem to be hard to find, this one might or might not work...
const WS_FQDN: &str = "websockets.chilkat.io";
const WS_PATH: &str = "/wsChilkatEcho.ashx";

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
    info!("About to open an HTTP connection to echo.websocket.org port 80");

    let ip = stack.get_host_by_name(WS_FQDN, AddrType::IPv4).await?;

    let mut conn: Connection<_> = Connection::new(buf, stack, SocketAddr::new(ip, 80));

    conn.initiate_ws_upgrade_request(
        Some(WS_FQDN),
        Some("foo.com"),
        WS_PATH,
        None,
        &[13; NONCE_LEN],
    )
    .await?;
    conn.initiate_response().await?;

    let headers = conn.headers()?;

    if headers.code != Some(101) {
        bail!("Unexpected status code: {:?}", headers.code);
    }

    conn.complete().await?;

    info!("Connection upgraded to WS, starting traffic now");

    let (mut tcp_conn, buf) = conn.release();

    let mut mask_source = thread_rng();

    for payload in ["Hello world!", "How are you?", "I'm fine, thanks!"] {
        let header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: payload.as_bytes().len() as _,
            mask_key: mask_source.next_u32().into(),
        };

        info!("Sending {header}, with payload \"{payload}\"");
        header.send(&mut tcp_conn).await?;
        header
            .send_payload(&mut tcp_conn, payload.as_bytes())
            .await?;

        let header = FrameHeader::recv(&mut tcp_conn).await?;
        header.recv_payload(&mut tcp_conn, buf).await?;

        match header.frame_type {
            FrameType::Text(_) => {
                info!(
                    "Got {header}, with payload \"{}\"",
                    core::str::from_utf8(buf[..header.payload_len as usize].as_ref()).unwrap()
                );
            }
            FrameType::Binary(_) => {
                info!("Got {header}, with payload {payload}");
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
