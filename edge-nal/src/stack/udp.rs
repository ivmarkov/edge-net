//! Factory traits for creating UDP sockets on embedded devices

use core::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use embedded_io_async::ErrorType;

use crate::udp::{UdpReceive, UdpSend};

/// A socket address that is completely unspecified (i.e. the IP address is unspecified, and the port is 0)
pub const UNSPECIFIED_ADDR: SocketAddr =
    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0));

/// This trait is implemented by UDP sockets that can be split into separate `send` and `receive` halves that can operate
/// independently from each other (i.e., a full-duplex connection).
///
/// All sockets returned by the `UdpStack` trait must implement this trait.
pub trait UdpSplit: ErrorType {
    type Receive<'a>: UdpReceive<Error = Self::Error>
    where
        Self: 'a;
    type Send<'a>: UdpSend<Error = Self::Error>
    where
        Self: 'a;

    fn split(&mut self) -> (Self::Receive<'_>, Self::Send<'_>);
}

impl<T> UdpSplit for &mut T
where
    T: UdpSplit,
{
    type Receive<'a> = T::Receive<'a> where Self: 'a;
    type Send<'a> = T::Send<'a> where Self: 'a;

    fn split(&mut self) -> (Self::Receive<'_>, Self::Send<'_>) {
        (**self).split()
    }
}

/// This trait is implemented by UDP/IP stacks. The trait allows the underlying driver to
/// construct multiple connections that implement the UDP traits from `edge-net`.
pub trait UdpStack {
    /// Error type returned on socket creation failure.
    type Error: embedded_io_async::Error;

    /// The socket type returned by the stack.
    type Socket<'a>: UdpReceive<Error = Self::Error>
        + UdpSend<Error = Self::Error>
        + UdpSplit<Error = Self::Error>
    where
        Self: 'a;

    /// Connect to a remote socket. Return the local socket address to which the connection is bound,
    /// as it might be only partially specified (as in either the port, or the IP address, or both might be unspecified).
    async fn connect(
        &self,
        local: SocketAddr,
        remote: SocketAddr,
    ) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error>;

    /// Bind to a local socket. Return the local socket address to which the connection is bound, as the provided
    /// local address might only be partially specified.
    async fn bind(&self, local: SocketAddr) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error>;
}

impl<T> UdpStack for &T
where
    T: UdpStack,
{
    type Error = T::Error;
    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn connect(
        &self,
        local: SocketAddr,
        remote: SocketAddr,
    ) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        (*self).connect(local, remote).await
    }

    async fn bind(&self, local: SocketAddr) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        (*self).bind(local).await
    }
}

impl<T> UdpStack for &mut T
where
    T: UdpStack,
{
    type Error = T::Error;
    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn connect(
        &self,
        local: SocketAddr,
        remote: SocketAddr,
    ) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        (**self).connect(local, remote).await
    }

    async fn bind(&self, local: SocketAddr) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        (**self).bind(local).await
    }
}
