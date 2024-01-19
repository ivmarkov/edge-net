use edge_raw::io::Udp2RawStack;
use edge_std_nal_async::StdRawStack;

use edge_dhcp::client::Client;
use edge_dhcp::io::{self, client::Lease, client::DEFAULT_CLIENT_PORT};

use embedded_nal_async::{Ipv4Addr, SocketAddrV4};

use log::info;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    futures_lite::future::block_on(run(
        2, // The interface index of the interface (e.g. eno0) to use; run `ip addr` to see it
        [0x4c, 0xcc, 0x6a, 0xa2, 0x23, 0xf5], // Your MAC addr here; run `ip addr` to see it
    ))
    .unwrap();
}

async fn run(if_index: u32, if_mac: [u8; 6]) -> Result<(), anyhow::Error> {
    let mut client = Client::new(rand::thread_rng(), if_mac);

    let stack: Udp2RawStack<_> = Udp2RawStack::new(StdRawStack::new(if_index));
    let mut buf = [0; 1500];

    loop {
        let mut socket = io::bind(
            &stack,
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_CLIENT_PORT),
        )
        .await?;

        let (mut lease, options) = Lease::new(&mut client, &mut socket, &mut buf).await?;

        info!("Got lease {lease:?} with options {options:?}");

        info!("Entering an endless loop to keep the lease...");

        lease.keep(&mut client, &mut socket, &mut buf).await?;
    }
}
