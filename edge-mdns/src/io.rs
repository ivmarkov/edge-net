use core::fmt;
use core::pin::pin;

use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::{NoopRawMutex, RawMutex};
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};

use embedded_nal_async::{
    IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpStack, UnconnectedUdp,
};
use embedded_nal_async_xtra::Multicast;

use log::{info, warn};

use self::split::{UdpSplit, UdpSplitReceive, UdpSplitSend};

use super::*;

pub use split::UdpSplitBuffer;

mod split;

pub const DEFAULT_SOCKET: SocketAddr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), PORT);

const IP_BROADCAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const IPV6_BROADCAST_ADDR: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0x00fb);

const PORT: u16 = 5353;

const MAX_TX_BUF_SIZE: usize = 1280 - 40/*IPV6 header size*/ - 8/*UDP header size*/;
const MAX_RX_BUF_SIZE: usize = 1583;

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

pub struct MdnsRunBuffers {
    tx_buf: core::mem::MaybeUninit<[u8; MAX_TX_BUF_SIZE]>,
    rx_buf: core::mem::MaybeUninit<[u8; MAX_RX_BUF_SIZE]>,
}

impl MdnsRunBuffers {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            tx_buf: core::mem::MaybeUninit::uninit(),
            rx_buf: core::mem::MaybeUninit::uninit(),
        }
    }
}

pub async fn run<'s, T, S>(
    host: &Host<'_>,
    interface: Option<u32>,
    services: T,
    stack: &S,
    socket: SocketAddr,
    udp_buffer: &mut UdpSplitBuffer,
    buffers: &mut MdnsRunBuffers,
) -> Result<(), MdnsIoError<S::Error>>
where
    T: IntoIterator<Item = Service<'s>> + Clone,
    S: UdpStack,
    S::UniquelyBound: Multicast<Error = S::Error>,
{
    let (local_addr, mut udp) = stack
        .bind_single(socket)
        .await
        .map_err(MdnsIoError::IoError)?;

    udp.join(IpAddr::V6(IPV6_BROADCAST_ADDR))
        .await
        .map_err(MdnsIoError::IoError)?;
    udp.join(IpAddr::V4(IP_BROADCAST_ADDR))
        .await
        .map_err(MdnsIoError::IoError)?;

    let mut split = UdpSplit::<NoopRawMutex, _>::new(udp, udp_buffer);

    let (send, recv) = split.split();

    let send_buf: &mut [u8] = unsafe { buffers.tx_buf.assume_init_mut() };
    let recv_buf = unsafe { buffers.rx_buf.assume_init_mut() };

    let send = Mutex::<NoopRawMutex, _>::new((send, send_buf));

    let mut broadcast = pin!(broadcast(
        host,
        services.clone(),
        local_addr,
        interface,
        &send
    ));
    let mut respond = pin!(respond(host, services, recv, recv_buf, &send));

    let result = select(&mut broadcast, &mut respond).await;

    match result {
        Either::First(result) => result,
        Either::Second(result) => result,
    }
}

async fn broadcast<'s, T, S, E>(
    host: &Host<'_>,
    services: T,
    local_addr: SocketAddr,
    interface: Option<u32>,
    send: &Mutex<impl RawMutex, (UdpSplitSend<'_, impl RawMutex, S>, &mut [u8])>,
) -> Result<(), MdnsIoError<E>>
where
    T: IntoIterator<Item = Service<'s>> + Clone,
    S: UnconnectedUdp<Error = E> + Multicast<Error = E>,
{
    loop {
        for remote_addr in
            core::iter::once(SocketAddr::V4(SocketAddrV4::new(IP_BROADCAST_ADDR, PORT))).chain(
                interface
                    .map(|interface| {
                        SocketAddr::V6(SocketAddrV6::new(IPV6_BROADCAST_ADDR, PORT, 0, interface))
                    })
                    .into_iter(),
            )
        {
            let mut guard = send.lock().await;
            let (send, send_buf) = &mut *guard;

            let len = host.broadcast(services.clone(), send_buf, 60)?;

            if len > 0 {
                info!("Broadcasting mDNS entry to {remote_addr}");
                send.send(local_addr, remote_addr, &send_buf[..len])
                    .await
                    .map_err(MdnsIoError::IoError)?;
            }
        }

        Timer::after(Duration::from_secs(30)).await;
    }
}

async fn respond<'s, T, S>(
    host: &Host<'_>,
    services: T,
    mut recv: UdpSplitReceive<'_, impl RawMutex, S>,
    recv_buf: &mut [u8],
    send: &Mutex<impl RawMutex, (UdpSplitSend<'_, impl RawMutex, S>, &mut [u8])>,
) -> Result<(), MdnsIoError<S::Error>>
where
    T: IntoIterator<Item = Service<'s>> + Clone,
    S: UnconnectedUdp,
{
    loop {
        let (len, local, remote) = recv
            .receive_into(recv_buf)
            .await
            .map_err(MdnsIoError::IoError)?;

        let mut guard = send.lock().await;
        let (send, send_buf) = &mut *guard;

        let len = match host.respond(services.clone(), &recv_buf[..len], send_buf, 60) {
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
            info!("Replying to mDNS query from {}", remote);

            send.send(local, remote, &send_buf[..len])
                .await
                .map_err(MdnsIoError::IoError)?;
        }
    }
}
