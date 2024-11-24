#![warn(clippy::large_futures)]

use core::net::SocketAddr;

use embedded_io_async::Read;

use edge_http::io::{client::Connection, Error};
use edge_http::Method;
use edge_nal::{AddrType, Dns, TcpConnect};

use log::*;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack: edge_nal_std::Stack = Default::default();

    let mut conn_buf = [0_u8; 8192];
    let mut data_buf = [0_u8; 1024];

    futures_lite::future::block_on(read(&stack, &mut conn_buf, &mut data_buf)).unwrap();
}

async fn read<T: TcpConnect + Dns>(
    stack: &T,
    conn_buf: &mut [u8],
    data_buf: &mut [u8],
) -> Result<(), Error<<T as TcpConnect>::Error>>
where
    <T as Dns>::Error: Into<<T as TcpConnect>::Error>,
{
    info!("About to open an HTTP connection to httpbin.org port 80");

    let ip = stack
        .get_host_by_name("httpbin.org", AddrType::IPv4)
        .await
        .map_err(|e| Error::Io(e.into()))?;

    let mut conn: Connection<_> = Connection::new(conn_buf, stack, SocketAddr::new(ip, 80));

    for uri in ["/ip", "/headers"] {
        request(&mut conn, uri, data_buf).await?;
    }

    Ok(())
}

async fn request<const N: usize, T: TcpConnect>(
    conn: &mut Connection<'_, T, N>,
    uri: &str,
    buf: &mut [u8],
) -> Result<(), Error<T::Error>> {
    conn.initiate_request(true, Method::Get, uri, &[("Host", "httpbin.org")])
        .await?;

    conn.initiate_response().await?;

    let mut result = Vec::new();

    loop {
        let len = conn.read(buf).await?;

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
