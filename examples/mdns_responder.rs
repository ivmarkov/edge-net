use core::net::{Ipv4Addr, Ipv6Addr};

use edge_mdns::buf::BufferAccess;
use edge_mdns::domain::base::Ttl;
use edge_mdns::io::{self, MdnsIoError, DEFAULT_SOCKET};
use edge_mdns::{host::Host, HostAnswersMdnsHandler};
use edge_nal::{UdpBind, UdpSplit};

use embassy_sync::blocking_mutex::raw::NoopRawMutex;

use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use log::*;

use rand::{thread_rng, RngCore};

// Change this to the IP address of the machine where you'll run this example
const OUR_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

const OUR_NAME: &str = "mypc";

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let stack = edge_nal_std::Stack::new();

    let (recv_buf, send_buf) = (
        Mutex::<NoopRawMutex, _>::new([0; 1500]),
        Mutex::<NoopRawMutex, _>::new([0; 1500]),
    );

    futures_lite::future::block_on(run::<edge_nal_std::Stack, _, _>(
        &stack, &recv_buf, &send_buf, OUR_NAME, OUR_IP,
    ))
    .unwrap();
}

async fn run<T, RB, SB>(
    stack: &T,
    recv_buf: RB,
    send_buf: SB,
    our_name: &str,
    our_ip: Ipv4Addr,
) -> Result<(), MdnsIoError<T::Error>>
where
    T: UdpBind,
    RB: BufferAccess,
    SB: BufferAccess,
    RB::BufferSurface: AsMut<[u8]>,
    SB::BufferSurface: AsMut<[u8]>,
{
    info!("About to run an mDNS responder for our PC. It will be addressable using {our_name}.local, so try to `ping {our_name}.local`.");

    let mut socket = io::bind(stack, DEFAULT_SOCKET, Some(Ipv4Addr::UNSPECIFIED), Some(0)).await?;

    let (recv, send) = socket.split();

    let host = Host {
        hostname: our_name,
        ipv4: our_ip,
        ipv6: Ipv6Addr::UNSPECIFIED,
        ttl: Ttl::from_secs(60),
    };

    // A way to notify the mDNS responder that the data in `Host` had changed
    // We don't use it in this example, because the data is hard-coded
    let signal = Signal::new();

    let mdns = io::Mdns::<NoopRawMutex, _, _, _, _>::new(
        Some(Ipv4Addr::UNSPECIFIED),
        Some(0),
        recv,
        send,
        recv_buf,
        send_buf,
        |buf| thread_rng().fill_bytes(buf),
        &signal,
    );

    mdns.run(HostAnswersMdnsHandler::new(&host)).await
}
