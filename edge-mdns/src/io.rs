use core::fmt;
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use core::pin::pin;

use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::{NoopRawMutex, RawMutex};
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;

use edge_nal::{UdpReceive, UdpSend};

use log::{info, warn};

use super::*;

pub const DEFAULT_SOCKET: SocketAddr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), PORT);

const IP_BROADCAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const IPV6_BROADCAST_ADDR: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0x00fb);

const PORT: u16 = 5353;

#[derive(Debug)]
pub enum MdnsIoError<E> {
    MdnsError(MdnsError),
    IoError(E),
}

impl<E> From<MdnsError> for MdnsIoError<E> {
    fn from(err: MdnsError) -> Self {
        Self::MdnsError(err)
    }
}

impl<E> fmt::Display for MdnsIoError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MdnsError(err) => write!(f, "mDNS error: {}", err),
            Self::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

#[cfg(feature = "std")]
impl<E> std::error::Error for MdnsIoError<E> where E: std::error::Error {}

/// Handles an incoming mDNS message by parsing it and potentially preparing a response.
/// 
/// If incoming is `None`, the handler should prepare a broadcast message with 
/// all its data.
/// 
/// Returns the length of the response message.
/// If length is 0, the IO layer using the handler should not send a message.
pub trait MdnsHandler {
    fn handle(&self, incoming: Option<&[u8]>, buf: &mut [u8], ttl_sec: u32) -> Result<usize, MdnsError>;
}

impl<T> MdnsHandler for &T
where
    T: MdnsHandler,
{
    fn handle(&self, incoming: Option<&[u8]>, buf: &mut [u8], ttl_sec: u32) -> Result<usize, MdnsError> {
        (**self).handle(incoming, buf, ttl_sec)
    }
}

/// An implementation of `Handler` based on information
/// captured in `Host` and `Service` structures.
pub struct HostHandler<'a, T> {
    host: &'a Host<'a>,
    services: T,
}

impl<'a, T> HostHandler<'a, T> {
    /// Create a new `HostResponder` with the given `Host` structure and services
    pub const fn new(host: &'a Host<'a>, services: T) -> Self {
        Self { host, services }
    }
}

impl<'a, T> MdnsHandler for HostHandler<'a, T>
where
    T: IntoIterator<Item = Service<'a>> + Clone,
{
    fn handle(&self, incoming: Option<&[u8]>, buf: &mut [u8], ttl_sec: u32) -> Result<usize, MdnsError> {
        if let Some(incoming) = incoming {
            self.host.respond(self.services.clone(), incoming, buf, ttl_sec)
        } else {
            self.host.broadcast(self.services.clone(), buf, ttl_sec)
        }
    }
}

pub async fn run<T, R, S>(
    handler: T,
    broadcast_signal: &Signal<impl RawMutex, ()>,
    ipv4_interface: Option<Ipv4Addr>,
    ipv6_interface: Option<u32>,
    recv: R,
    send: S,
    recv_buf: &mut [u8],
    send_buf: &mut [u8],
) -> Result<(), MdnsIoError<S::Error>>
where
    T: MdnsHandler,
    R: UdpReceive,
    S: UdpSend<Error = R::Error>,
{
    // let mut udp = stack.bind(socket).await.map_err(MdnsIoError::IoError)?;

    // if let Some(v4) = ipv4_interface {
    //     udp.join_v4(IP_BROADCAST_ADDR, v4)
    //         .await
    //         .map_err(MdnsIoError::IoError)?;
    // }

    // if let Some(v6) = ipv6_interface {
    //     udp.join_v6(IPV6_BROADCAST_ADDR, v6)
    //         .await
    //         .map_err(MdnsIoError::IoError)?;
    // }

    let send = Mutex::<NoopRawMutex, _>::new((send, send_buf));

    let mut broadcast = pin!(broadcast(
        &handler, 
        broadcast_signal,
        ipv4_interface.is_some(),
        ipv6_interface,
        &send
    ));
    let mut respond = pin!(respond(&handler, recv, recv_buf, &send));

    let result = select(&mut broadcast, &mut respond).await;

    match result {
        Either::First(result) => result,
        Either::Second(result) => result,
    }
}

async fn broadcast<T, S>(
    handler: T,
    broadcast_signal: &Signal<impl RawMutex, ()>,
    ipv4: bool,
    ipv6_interface: Option<u32>,
    send: &Mutex<impl RawMutex, (S, &mut [u8])>,
) -> Result<(), MdnsIoError<S::Error>>
where
    T: MdnsHandler,
    S: UdpSend,
{
    loop {
        for remote_addr in
            core::iter::once(SocketAddr::V4(SocketAddrV4::new(IP_BROADCAST_ADDR, PORT)))
                .filter(|_| ipv4)
                .chain(
                    ipv6_interface
                        .map(|interface| {
                            SocketAddr::V6(SocketAddrV6::new(
                                IPV6_BROADCAST_ADDR,
                                PORT,
                                0,
                                interface,
                            ))
                        })
                        .into_iter(),
                )
        {
            let mut guard = send.lock().await;
            let (send, send_buf) = &mut *guard;

            let len = handler.handle(None, send_buf, 60)?;

            if len > 0 {
                info!("Broadcasting mDNS entry to {remote_addr}");

                let fut = pin!(send.send(remote_addr, &send_buf[..len]));

                fut.await.map_err(MdnsIoError::IoError)?;
            }
        }

        broadcast_signal.wait().await;
    }
}

async fn respond<T, R, S>(
    responder: T,
    mut recv: R,
    recv_buf: &mut [u8],
    send: &Mutex<impl RawMutex, (S, &mut [u8])>,
) -> Result<(), MdnsIoError<S::Error>>
where
    T: MdnsHandler,
    R: UdpReceive,
    S: UdpSend<Error = R::Error>,
{
    loop {
        let (len, remote) = recv.receive(recv_buf).await.map_err(MdnsIoError::IoError)?;

        let mut guard = send.lock().await;
        let (send, send_buf) = &mut *guard;

        let len = match responder.handle(Some(&recv_buf[..len]), send_buf, 60) {
            Ok(len) => len,
            Err(err) => match err {
                MdnsError::InvalidMessage => {
                    warn!("Got invalid message from {remote}, skipping");
                    continue;
                }
                other => Err(other)?,
            },
        };

        if len > 0 {
            info!("Replying to mDNS query from {remote}");

            let fut = pin!(send.send(remote, &send_buf[..len]));

            match fut.await {
                Ok(_) => (),
                Err(err) => {
                    // Turns out we might receive queries from Ipv6 addresses which are actually unreachable by us
                    // Still to be investigated why, but it does seem that we are receiving packets which contain
                    // non-link-local Ipv6 addresses, to which we cannot respond
                    //
                    // A possible reason for this might be that we are receiving these packets via the broadcast group
                    // - yet - it is still unclear how these arrive given that we are only listening on the link-local address
                    warn!("IO error {err:?} while replying to {remote}");
                }
            }
        }
    }
}
