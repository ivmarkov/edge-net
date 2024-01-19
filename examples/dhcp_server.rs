/// NOTE: Run this example with `sudo` to be able to bind to the interface, as it uses raw sockets which require root privileges.
use edge_raw::io::Udp2RawStack;
use edge_std_nal_async::StdRawStack;

use edge_dhcp::io::{self, DEFAULT_SERVER_PORT};
use edge_dhcp::server::{Server, ServerOptions};

use embedded_nal_async::{Ipv4Addr, SocketAddrV4};

fn main() {
    futures_lite::future::block_on(run(
        0, // The interface index of the interface (e.g. eno0) to use; run `ip addr` to see it
    ))
    .unwrap();
}

async fn run(if_index: u32) -> Result<(), anyhow::Error> {
    let stack: Udp2RawStack<_> = Udp2RawStack::new(StdRawStack::new(if_index));

    let mut buf = [0; 1500];

    let ip = Ipv4Addr::new(192, 168, 0, 1);

    let mut socket = io::bind(&stack, SocketAddrV4::new(ip, DEFAULT_SERVER_PORT)).await?;

    let mut gw_buf = [Ipv4Addr::UNSPECIFIED];

    io::server::run(
        &mut Server::<64>::new(ip), // Will give IP addresses in the range 192.168.0.50 - 192.168.0.200, subnet 255.255.255.0
        &ServerOptions::new(ip, Some(&mut gw_buf)),
        &mut socket,
        &mut buf,
    )
    .await?;

    Ok(())
}
