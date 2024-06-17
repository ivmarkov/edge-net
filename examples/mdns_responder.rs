use core::net::Ipv4Addr;

use edge_mdns::domain::base::Ttl;
use edge_mdns::io::{self, MdnsIoError, DEFAULT_SOCKET};
use edge_mdns::{host::Host, HostAnswersMdnsHandler};
use edge_nal::{UdpBind, UdpSplit};

use embassy_sync::blocking_mutex::raw::NoopRawMutex;

use log::*;

// Change this to the IP address of the machine where you'll run this example
const OUR_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

const OUR_NAME: &str = "mypc";

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack = edge_nal_std::Stack::new();

    let (mut recv_buf, mut send_buf) = ([0; 1500], [0; 1500]);

    futures_lite::future::block_on(run::<edge_nal_std::Stack>(
        &stack,
        &mut recv_buf,
        &mut send_buf,
        OUR_NAME,
        OUR_IP,
    ))
    .unwrap();
}

async fn run<T>(
    stack: &T,
    recv_buf: &mut [u8],
    send_buf: &mut [u8],
    our_name: &str,
    our_ip: Ipv4Addr,
) -> Result<(), MdnsIoError<T::Error>>
where
    T: UdpBind,
{
    info!("About to run an mDNS responder for our PC. It will be addressable using {our_name}.local, so try to `ping {our_name}.local`.");

    let mut socket = io::bind(stack, DEFAULT_SOCKET, Some(Ipv4Addr::UNSPECIFIED), Some(0)).await?;

    let (recv, send) = socket.split();

    let host = Host {
        hostname: our_name,
        ip: our_ip,
        ipv6: None,
        ttl: Ttl::from_secs(60),
    };

    let mdns = io::Mdns::<NoopRawMutex, _, _, _>::new(
        HostAnswersMdnsHandler::new(&host),
        Some(Ipv4Addr::UNSPECIFIED),
        Some(0),
        recv,
        recv_buf,
        send,
        send_buf,
    );

    mdns.run().await
}
