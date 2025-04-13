use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use core::ptr::NonNull;

use edge_nal::{MulticastV4, MulticastV6, Readable, UdpBind, UdpReceive, UdpSend, UdpSplit};

use embassy_net::udp::{BindError, PacketMetadata, RecvError, SendError};
use embassy_net::Stack;

use embedded_io_async::{ErrorKind, ErrorType};

use crate::{to_emb_bind_socket, to_emb_socket, to_net_socket, Pool};

/// A struct that implements the `UdpBind` factory trait from `edge-nal`
/// Capable of managing up to N concurrent connections with TX and RX buffers according to TX_SZ and RX_SZ, and packet metadata according to `M`.
pub struct Udp<
    'd,
    const N: usize,
    const TX_SZ: usize = 1500,
    const RX_SZ: usize = 1500,
    const M: usize = 2,
> {
    stack: Stack<'d>,
    buffers: &'d UdpBuffers<N, TX_SZ, RX_SZ, M>,
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize>
    Udp<'d, N, TX_SZ, RX_SZ, M>
{
    /// Create a new `Udp` instance for the provided Embassy networking stack using the provided UDP buffers.
    ///
    /// Ensure that the number of buffers `N` fits within StackResources<N> of
    /// [embassy_net::Stack], while taking into account the sockets used for DHCP, DNS, etc. else
    /// [smoltcp::iface::SocketSet] will panic with `adding a socket to a full SocketSet`.
    pub fn new(stack: Stack<'d>, buffers: &'d UdpBuffers<N, TX_SZ, RX_SZ, M>) -> Self {
        Self { stack, buffers }
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> UdpBind
    for Udp<'_, N, TX_SZ, RX_SZ, M>
{
    type Error = UdpError;

    type Socket<'a>
        = UdpSocket<'a, N, TX_SZ, RX_SZ, M>
    where
        Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        let mut socket = UdpSocket::new(self.stack, self.buffers)?;

        socket
            .socket
            .bind(to_emb_bind_socket(local).ok_or(UdpError::UnsupportedProto)?)?;

        Ok(socket)
    }
}

/// A UDP socket
/// Implements the `UdpReceive` `UdpSend` and `UdpSplit` traits from `edge-nal`
pub struct UdpSocket<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> {
    #[allow(unused)]
    stack: embassy_net::Stack<'d>,
    socket: embassy_net::udp::UdpSocket<'d>,
    stack_buffers: &'d UdpBuffers<N, TX_SZ, RX_SZ, M>,
    socket_buffers: NonNull<([u8; TX_SZ], [u8; RX_SZ])>,
    socket_meta_buffers: NonNull<([PacketMetadata; M], [PacketMetadata; M])>,
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize>
    UdpSocket<'d, N, TX_SZ, RX_SZ, M>
{
    fn new(
        stack: Stack<'d>,
        stack_buffers: &'d UdpBuffers<N, TX_SZ, RX_SZ, M>,
    ) -> Result<Self, UdpError> {
        let mut socket_buffers = stack_buffers.pool.alloc().ok_or(UdpError::NoBuffers)?;
        let mut socket_meta_buffers = unwrap!(stack_buffers.meta_pool.alloc());

        Ok(Self {
            stack,
            socket: unsafe {
                embassy_net::udp::UdpSocket::new(
                    stack,
                    &mut socket_meta_buffers.as_mut().1,
                    &mut socket_buffers.as_mut().1,
                    &mut socket_meta_buffers.as_mut().0,
                    &mut socket_buffers.as_mut().0,
                )
            },
            stack_buffers,
            socket_buffers,
            socket_meta_buffers,
        })
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> Drop
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    fn drop(&mut self) {
        unsafe {
            self.socket.close();
            self.stack_buffers.pool.free(self.socket_buffers);
            self.stack_buffers.meta_pool.free(self.socket_meta_buffers);
        }
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> ErrorType
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    type Error = UdpError;
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> UdpReceive
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, SocketAddr), Self::Error> {
        let (len, remote_endpoint) = self.socket.recv_from(buffer).await?;

        Ok((len, to_net_socket(remote_endpoint.endpoint)))
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> UdpSend
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn send(&mut self, remote: SocketAddr, data: &[u8]) -> Result<(), Self::Error> {
        self.socket
            .send_to(
                data,
                to_emb_socket(remote).ok_or(UdpError::UnsupportedProto)?,
            )
            .await?;

        Ok(())
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> ErrorType
    for &UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    type Error = UdpError;
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> UdpReceive
    for &UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, SocketAddr), Self::Error> {
        let (len, remote_endpoint) = self.socket.recv_from(buffer).await?;

        Ok((len, to_net_socket(remote_endpoint.endpoint)))
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> UdpSend
    for &UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn send(&mut self, remote: SocketAddr, data: &[u8]) -> Result<(), Self::Error> {
        self.socket
            .send_to(
                data,
                to_emb_socket(remote).ok_or(UdpError::UnsupportedProto)?,
            )
            .await?;

        Ok(())
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> Readable
    for &UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn readable(&mut self) -> Result<(), Self::Error> {
        self.socket.wait_recv_ready().await;
        Ok(())
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> UdpSplit
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    type Receive<'a>
        = &'a Self
    where
        Self: 'a;

    type Send<'a>
        = &'a Self
    where
        Self: 'a;

    fn split(&mut self) -> (Self::Receive<'_>, Self::Send<'_>) {
        (&*self, &*self)
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> MulticastV4
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn join_v4(
        &mut self,
        #[allow(unused)] multicast_addr: Ipv4Addr,
        _interface: Ipv4Addr,
    ) -> Result<(), Self::Error> {
        #[cfg(feature = "multicast")]
        {
            self.stack.join_multicast_group(
                crate::to_emb_addr(core::net::IpAddr::V4(multicast_addr))
                    .ok_or(UdpError::UnsupportedProto)?,
            )?;
        }

        #[cfg(not(feature = "multicast"))]
        {
            Err(UdpError::UnsupportedProto)?;
        }

        Ok(())
    }

    async fn leave_v4(
        &mut self,
        #[allow(unused)] multicast_addr: Ipv4Addr,
        _interface: Ipv4Addr,
    ) -> Result<(), Self::Error> {
        #[cfg(feature = "multicast")]
        {
            self.stack.leave_multicast_group(
                crate::to_emb_addr(core::net::IpAddr::V4(multicast_addr))
                    .ok_or(UdpError::UnsupportedProto)?,
            )?;
        }

        #[cfg(not(feature = "multicast"))]
        {
            Err(UdpError::UnsupportedProto)?;
        }

        Ok(())
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> MulticastV6
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn join_v6(
        &mut self,
        #[allow(unused)] multicast_addr: Ipv6Addr,
        _interface: u32,
    ) -> Result<(), Self::Error> {
        #[cfg(feature = "multicast")]
        {
            self.stack.join_multicast_group(
                crate::to_emb_addr(core::net::IpAddr::V6(multicast_addr))
                    .ok_or(UdpError::UnsupportedProto)?,
            )?;
        }

        #[cfg(not(feature = "multicast"))]
        {
            Err(UdpError::UnsupportedProto)?;
        }

        Ok(())
    }

    async fn leave_v6(
        &mut self,
        #[allow(unused)] multicast_addr: Ipv6Addr,
        _interface: u32,
    ) -> Result<(), Self::Error> {
        #[cfg(feature = "multicast")]
        {
            self.stack.leave_multicast_group(
                crate::to_emb_addr(core::net::IpAddr::V6(multicast_addr))
                    .ok_or(UdpError::UnsupportedProto)?,
            )?;
        }

        #[cfg(not(feature = "multicast"))]
        {
            Err(UdpError::UnsupportedProto)?;
        }

        Ok(())
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> Readable
    for UdpSocket<'_, N, TX_SZ, RX_SZ, M>
{
    async fn readable(&mut self) -> Result<(), Self::Error> {
        self.socket.wait_recv_ready().await;
        Ok(())
    }
}

/// A shared error type that is used by the UDP factory trait implementation as well as the UDP socket
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum UdpError {
    Recv(RecvError),
    Send(SendError),
    Bind(BindError),
    /// The table of joined multicast groups is already full.
    MulticastGroupTableFull,
    /// Cannot join/leave the given multicast group.
    MulticastUnaddressable,
    NoBuffers,
    UnsupportedProto,
}

impl From<RecvError> for UdpError {
    fn from(e: RecvError) -> Self {
        UdpError::Recv(e)
    }
}

impl From<SendError> for UdpError {
    fn from(e: SendError) -> Self {
        UdpError::Send(e)
    }
}

impl From<BindError> for UdpError {
    fn from(e: BindError) -> Self {
        UdpError::Bind(e)
    }
}

#[cfg(all(
    feature = "multicast",
    any(feature = "proto-ipv4", feature = "proto-ipv6")
))]
impl From<embassy_net::MulticastError> for UdpError {
    fn from(e: embassy_net::MulticastError) -> Self {
        match e {
            embassy_net::MulticastError::GroupTableFull => UdpError::MulticastGroupTableFull,
            embassy_net::MulticastError::Unaddressable => UdpError::MulticastUnaddressable,
        }
    }
}

// TODO
impl embedded_io_async::Error for UdpError {
    fn kind(&self) -> ErrorKind {
        match self {
            UdpError::Recv(_) => ErrorKind::Other,
            UdpError::Send(_) => ErrorKind::Other,
            UdpError::Bind(_) => ErrorKind::Other,
            UdpError::MulticastGroupTableFull => ErrorKind::Other,
            UdpError::MulticastUnaddressable => ErrorKind::Other,
            UdpError::NoBuffers => ErrorKind::OutOfMemory,
            UdpError::UnsupportedProto => ErrorKind::InvalidInput,
        }
    }
}

/// A struct that holds a pool of UDP buffers
pub struct UdpBuffers<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> {
    pool: Pool<([u8; TX_SZ], [u8; RX_SZ]), N>,
    meta_pool: Pool<
        (
            [embassy_net::udp::PacketMetadata; M],
            [embassy_net::udp::PacketMetadata; M],
        ),
        N,
    >,
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize> Default
    for UdpBuffers<N, TX_SZ, RX_SZ, M>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize, const M: usize>
    UdpBuffers<N, TX_SZ, RX_SZ, M>
{
    /// Create a new `UdpBuffers` instance
    pub const fn new() -> Self {
        Self {
            pool: Pool::new(),
            meta_pool: Pool::new(),
        }
    }
}
