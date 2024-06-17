use core::cell::RefCell;
use core::fmt;
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use core::pin::pin;

use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex;
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;

use edge_nal::{MulticastV4, MulticastV6, UdpBind, UdpReceive, UdpSend};

use log::{info, warn};

use super::*;

/// A quick-and-dirty socket address that binds to a "default" interface.
/// Don't use in production code.
pub const DEFAULT_SOCKET: SocketAddr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), PORT);

/// The IPv4 mDNS broadcast address, as per spec.
pub const IP_BROADCAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
/// The IPv6 mDNS broadcast address, as per spec.
pub const IPV6_BROADCAST_ADDR: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0x00fb);

/// The mDNS port, as per spec.
pub const PORT: u16 = 5353;

/// A wrapper for mDNS and IO errors.
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

/// A utility method to bind a socket suitable for mDNS, by using the provided
/// stack and address, and optionally joining the provided interfaces via multicast.
///
/// Note that mDNS is pointless without multicast, so at least one - or both - of the
/// ipv4 and ipv6 interfaces need to be provided.
pub async fn bind<S>(
    stack: &S,
    addr: SocketAddr,
    ipv4_interface: Option<Ipv4Addr>,
    ipv6_interface: Option<u32>,
) -> Result<S::Socket<'_>, MdnsIoError<S::Error>>
where
    S: UdpBind,
{
    let mut socket = stack.bind(addr).await.map_err(MdnsIoError::IoError)?;

    if let Some(v4) = ipv4_interface {
        socket
            .join_v4(IP_BROADCAST_ADDR, v4)
            .await
            .map_err(MdnsIoError::IoError)?;
    }

    if let Some(v6) = ipv6_interface {
        socket
            .join_v6(IPV6_BROADCAST_ADDR, v6)
            .await
            .map_err(MdnsIoError::IoError)?;
    }

    Ok(socket)
}

/// Represents an mDNS service that can respond to queries using the provided handler.
///
/// This structure is generic over the mDNS handler, the UDP receiver and sender, and the
/// raw mutex type.
///
/// The handler is expected to be a type that implements the `MdnsHandler` trait, which
/// allows it to handle mDNS queries and generate responses, as well as to handle mDNS
/// responses to queries which we might have issues using the `query` method.
pub struct Mdns<'a, M, T, R, S>
where
    M: RawMutex,
{
    handler: blocking_mutex::Mutex<M, RefCell<T>>,
    broadcast_signal: Signal<M, ()>,
    ipv4_interface: Option<Ipv4Addr>,
    ipv6_interface: Option<u32>,
    recv: Mutex<M, (R, &'a mut [u8])>,
    send: Mutex<M, (S, &'a mut [u8])>,
}

impl<'a, T, R, S, M> Mdns<'a, M, T, R, S>
where
    M: RawMutex,
    T: MdnsHandler,
    R: UdpReceive,
    S: UdpSend<Error = R::Error>,
{
    /// Creates a new mDNS service with the provided handler, interfaces, and UDP receiver and sender.
    pub fn new(
        handler: T,
        ipv4_interface: Option<Ipv4Addr>,
        ipv6_interface: Option<u32>,
        recv: R,
        recv_buf: &'a mut [u8],
        send: S,
        send_buf: &'a mut [u8],
    ) -> Self {
        Self {
            handler: blocking_mutex::Mutex::new(RefCell::new(handler)),
            broadcast_signal: Signal::new(),
            ipv4_interface,
            ipv6_interface,
            recv: Mutex::new((recv, recv_buf)),
            send: Mutex::new((send, send_buf)),
        }
    }

    /// Runs the mDNS service, handling queries and responding to them, as well as broadcasting
    /// mDNS answers and handling responses to our own queries.
    ///
    /// All of the handling logic is expected to be implemented in the handler provided when
    /// creating the mDNS service.
    ///
    /// I.e. hanbdling responses to our own queries cannot happen, unless the supplied handler
    /// is capable of doing that (i.e. it is a `PeerMdnsHandler`, or a chain containing it, or similar).
    ///
    /// Ditto for handling queries coming from other peers - this can only happen if the handler
    /// is capable of doing that. I.e., it is a `HostMdnsHandler`, or a chain containing it, or similar.
    pub async fn run(&self) -> Result<(), MdnsIoError<S::Error>> {
        let mut broadcast = pin!(self.broadcast());
        let mut respond = pin!(self.respond());

        let result = select(&mut broadcast, &mut respond).await;

        match result {
            Either::First(result) => result,
            Either::Second(result) => result,
        }
    }

    /// Sends a multicast query with the provided payload.
    /// It is assumed that the payload represents a valid mDNS query message.
    ///
    /// The payload is constructed via a closure, because this way we can provide to
    /// the payload-constructing closure a ready-to-use `&mut [u8]` slice, where the
    /// closure can arrange the mDNS query message (i.e. we avoid extra memory usage
    /// by constructing the mDNS query directly in the `send_buf` buffer that was supplied
    /// when the `Mdns` instance was constructed).
    pub async fn query<Q>(&self, q: Q) -> Result<(), MdnsIoError<S::Error>>
    where
        Q: FnOnce(&mut [u8]) -> Result<usize, MdnsError>,
    {
        let mut guard = self.send.lock().await;
        let (send, send_buf) = &mut *guard;

        let len = q(send_buf)?;

        if len > 0 {
            self.broadcast_once(send, &send_buf[..len], true, true)
                .await?;
        }

        Ok(())
    }

    /// Notifies the mDNS service that the answers have changed, and that it should
    /// broadcast the new answers.
    pub fn notify_answers_changed(&self) {
        self.broadcast_signal.signal(());
    }

    async fn broadcast(&self) -> Result<(), MdnsIoError<S::Error>> {
        loop {
            let mut guard = self.send.lock().await;
            let (send, send_buf) = &mut *guard;

            let len = self
                .handler
                .lock(|handler| handler.borrow_mut().handle(None, send_buf))?;

            if len > 0 {
                self.broadcast_once(send, &send_buf[..len], true, true)
                    .await?;
            }

            self.broadcast_signal.wait().await;
        }
    }

    async fn respond(&self) -> Result<(), MdnsIoError<S::Error>> {
        let mut guard = self.recv.lock().await;
        let (recv, recv_buf) = &mut *guard;

        loop {
            let (len, remote) = recv.receive(recv_buf).await.map_err(MdnsIoError::IoError)?;

            info!("Got mDNS query from {remote}");

            let mut guard = self.send.lock().await;
            let (send, send_buf) = &mut *guard;

            let len = match self.handler.lock(|handler| {
                handler
                    .borrow_mut()
                    .handle(Some(&recv_buf[..len]), send_buf)
            }) {
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

                self.broadcast_once(
                    send,
                    &send_buf[..len],
                    matches!(remote, SocketAddr::V4(_)),
                    matches!(remote, SocketAddr::V6(_)),
                )
                .await?;
            }
        }
    }

    async fn broadcast_once(
        &self,
        send: &mut S,
        data: &[u8],
        ipv4: bool,
        ipv6: bool,
    ) -> Result<(), MdnsIoError<S::Error>> {
        for remote_addr in
            core::iter::once(SocketAddr::V4(SocketAddrV4::new(IP_BROADCAST_ADDR, PORT)))
                .filter(|_| ipv4 && self.ipv4_interface.is_some())
                .chain(
                    self.ipv6_interface
                        .map(|interface| {
                            SocketAddr::V6(SocketAddrV6::new(
                                IPV6_BROADCAST_ADDR,
                                PORT,
                                0,
                                interface,
                            ))
                        })
                        .into_iter()
                        .filter(|_| ipv6),
                )
        {
            if !data.is_empty() {
                info!("Broadcasting mDNS entry to {remote_addr}");

                let fut = pin!(send.send(remote_addr, data));

                fut.await.map_err(MdnsIoError::IoError)?;
            }
        }

        Ok(())
    }
}
