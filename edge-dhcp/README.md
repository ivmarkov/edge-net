# edge-dhcp

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

Async + `no_std` + no-alloc implementation of the DHCP protocol.

## Examples

### DHCP client

```rust
use edge_raw::{StdRawStack, Udp2RawStack};
use edge_dhcp::{DEFAULT_CLIENT_PORT, Client, io::Lease, io::Error};

use embedded_nal_async::Ipv4Addr;

fn main() {
    futures_lite::task::block_on(0, run([0; 6]/* Your MAC addr here */)).unwrap();
}

async fn run(if_index: u8, if_mac: [u8; 6]) -> Result<(), impl Debug> {
    let mut client = Client::new(thread_rng(), if_mac);

    let stack: Udp2RawStack<_> = Udp2RawStack::new(StdRawStack::new(if_index));
    let mut buf = [0; 1500];

    loop {
        let mut socket = bind(
            &stack,
            SocketAddrV4::new(
                Ipv4Addr::UNSPECIFIED,
                DEFAULT_CLIENT_PORT,
            ),
        )
        .await?;

        let (mut lease, options) =
            Lease::new(&mut client, &mut socket, &mut buf).await?;

        info!("Got lease {lease:?} with options {options:?}");

        lease.keep(&mut client, &mut socket, &mut buf).await?;
    }
}
```

### DHCP server

```rust
use edge_raw::{StdRawStack, Udp2RawStack};
use edge_dhcp::{self, DEFAULT_SERVER_PORT, Server, ServerOptions, io::Error};

use embedded_nal_async::{Ipv4Addr, SocketAddrV4};

fn main() {
    futures_lite::task::block_on(run(0)).unwrap();
}

async fn run(if_index: u8) -> Result<(), impl Debug> {
    let stack: Udp2RawStack<_> = Udp2RawStack::new(StdRawStack::new(if_index));

    let mut buf = [0; 1500];

    let ip = Ipv4Addr::new(192, 168, 0, 1);

    let mut socket = io::bind(&stack, SocketAddrV4::new(ip, DEFAULT_SERVER_PORT)).await?;

    let mut gw_buf = [Ipv4Addr::UNSPECIFIED];

    io::run(
        &mut Server::<64>::new(ip), // Will give IP addresses in the rage 192.168.0.50 - 192.168.0.200, subnet 255.255.255.0
        &ServerOptions::new(ip, Some(&mut gw_buf)),
        &mut socket,
        &mut buf,
    )
    .await
}
```
