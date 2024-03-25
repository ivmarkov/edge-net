use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use embedded_io_async::{Read, Write};

use edge_nal::TcpConnect;

use log::*;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack = edge_nal_std::Stack::new();

    futures_lite::future::block_on(read(&stack)).unwrap();
}

async fn read<T: TcpConnect>(stack: &T) -> Result<(), T::Error> {
    info!("About to open a TCP connection to 1.1.1.1 port 80");

    let mut connection = stack
        .connect(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 80))
        .await?
        .1;

    connection.write_all(b"GET / HTTP/1.0\n\n").await?;

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
        "1.1.1.1 returned:\n=================\n{}\n=================\nSince it returned something, all seems OK!",
        core::str::from_utf8(&result).unwrap());

    Ok(())
}
