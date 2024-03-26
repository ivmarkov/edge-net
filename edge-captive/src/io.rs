use core::fmt;
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use core::time::Duration;

use edge_nal::{UdpBind, UdpReceive, UdpSend};

use log::*;

use super::*;

pub const DEFAULT_SOCKET: SocketAddr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), PORT);

const PORT: u16 = 53;

#[derive(Debug)]
pub enum DnsIoError<E> {
    DnsError(DnsError),
    IoError(E),
}

impl<E> From<DnsError> for DnsIoError<E> {
    fn from(err: DnsError) -> Self {
        Self::DnsError(err)
    }
}

impl<E> fmt::Display for DnsIoError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DnsError(err) => write!(f, "DNS error: {}", err),
            Self::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

#[cfg(feature = "std")]
impl<E> std::error::Error for DnsIoError<E> where E: std::error::Error {}

pub async fn run<S>(
    stack: &S,
    local_addr: SocketAddr,
    tx_buf: &mut [u8],
    rx_buf: &mut [u8],
    ip: Ipv4Addr,
    ttl: Duration,
) -> Result<(), DnsIoError<S::Error>>
where
    S: UdpBind,
{
    let mut udp = stack.bind(local_addr).await.map_err(DnsIoError::IoError)?;

    loop {
        debug!("Waiting for data");

        let (len, remote) = udp.receive(rx_buf).await.map_err(DnsIoError::IoError)?;

        let request = &rx_buf[..len];

        debug!("Received {} bytes from {remote}", request.len());

        let len = match crate::reply(request, &ip.octets(), ttl, tx_buf) {
            Ok(len) => len,
            Err(err) => match err {
                DnsError::InvalidMessage => {
                    warn!("Got invalid message from {remote}, skipping");
                    continue;
                }
                other => Err(other)?,
            },
        };

        udp.send(remote, &tx_buf[..len])
            .await
            .map_err(DnsIoError::IoError)?;

        debug!("Sent {len} bytes to {remote}");
    }
}
