use embedded_nal_async::{Ipv4Addr, UdpStack};
use embedded_nal_async_xtra::Multicast;

use edge_mdns::io::{self, MdnsIoError, MdnsRunBuffers, UdpSplitBuffer, DEFAULT_SOCKET};
use edge_mdns::Host;

use edge_std_nal_async::StdUdpStack;

use log::*;

// Change this to the IP address of the machine where you'll run this example
const OUR_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

const OUR_NAME: &str = "mypc";

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack = StdUdpStack::new();

    let mut udp_buffer = UdpSplitBuffer::new();
    let mut buffers = MdnsRunBuffers::new();

    futures_lite::future::block_on(run(&stack, &mut udp_buffer, &mut buffers, OUR_NAME, OUR_IP))
        .unwrap();
}

async fn run<T: UdpStack>(
    stack: &T,
    udp_buffer: &mut UdpSplitBuffer,
    buffers: &mut MdnsRunBuffers,
    our_name: &str,
    our_ip: Ipv4Addr,
) -> Result<(), MdnsIoError<T::Error>>
where
    T: UdpStack,
    <T as UdpStack>::UniquelyBound: Multicast<Error = T::Error>,
{
    info!("About to run an mDNS responder for our PC. It will be addressable using {our_name}.local, so try to `ping {our_name}.local`.");

    let host = Host {
        id: 0,
        hostname: our_name,
        ip: our_ip.octets(),
        ipv6: None,
    };

    io::run(
        &host,
        Some(0),
        [],
        stack,
        DEFAULT_SOCKET,
        udp_buffer,
        buffers,
    )
    .await
}
