use embedded_io_async::Read;
use embedded_nal_async::{AddrType, Dns, SocketAddr, TcpConnect};

use edge_net::http::io::{client::ClientConnection, Error};
use edge_net::http::Method;

use std_embedded_nal_async::Stack;

use log::*;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack: Stack = Default::default();

    futures_lite::future::block_on(read(&stack)).unwrap();
}

async fn read<T: TcpConnect + Dns>(stack: &T) -> Result<(), Error<<T as TcpConnect>::Error>>
where
    <T as Dns>::Error: Into<<T as TcpConnect>::Error>,
{
    info!("About to open an HTTP connection to httpbin.org port 80");

    let ip = stack
        .get_host_by_name("httpbin.org", AddrType::IPv4)
        .await
        .map_err(|e| Error::Io(e.into()))?;

    let mut buf = [0_u8; 8192];

    let mut connection = ClientConnection::<1024, _>::new(&mut buf, stack, SocketAddr::new(ip, 80));

    for uri in ["/ip", "/headers"] {
        request(&mut connection, uri).await?;
    }

    Ok(())
}

async fn request<'b, const N: usize, T: TcpConnect>(
    connection: &mut ClientConnection<'b, N, T>,
    uri: &str,
) -> Result<(), Error<T::Error>> {
    connection
        .initiate_request(true, Method::Get, uri, &[("Host", "httpbin.org")])
        .await?;
    connection.initiate_response().await?;

    let mut result = Vec::new();

    let mut buf = [0_u8; 1024];

    loop {
        let len = connection.read(&mut buf).await?;

        if len > 0 {
            result.extend_from_slice(&buf[0..len]);
        } else {
            break;
        }
    }

    info!(
        "Request to httpbin.org, URI \"{}\" returned:\nBody:\n=================\n{}\n=================\n\n\n\n",
        uri,
        //response.headers(),
        core::str::from_utf8(&result).unwrap());

    Ok(())
}
