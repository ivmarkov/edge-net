//! Factory traits for creating TCP sockets on embedded devices

use core::net::SocketAddr;

use embedded_io_async::{Error, ErrorType, Read, Write};

/// This trait is implemented by TCP sockets that can be split into separate `send` and `receive` halves that can operate
/// independently from each other (i.e., a full-duplex connection).
pub trait TcpSplit: ErrorType {
    type Read<'a>: Read<Error = Self::Error>
    where
        Self: 'a;
    type Write<'a>: Write<Error = Self::Error>
    where
        Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>);
}

impl<T> TcpSplit for &mut T
where
    T: TcpSplit,
{
    type Read<'a> = T::Read<'a> where Self: 'a;
    type Write<'a> = T::Write<'a> where Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        (**self).split()
    }
}

/// This is a factory trait for connecting to remote TCP peers
pub trait TcpConnect {
    /// Error type returned on socket creation failure
    type Error: Error;

    /// The socket type returned by the factory
    type Socket<'a>: Read<Error = Self::Error> + Write<Error = Self::Error>
    where
        Self: 'a;

    /// Connect to a remote socket
    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error>;
}

/// This is a factory trait for creating server-side TCP sockets
pub trait TcpBind {
    /// Error type returned on bind failure
    type Error: Error;

    /// The acceptor type returned by the factory
    type Accept<'a>: TcpAccept<Error = Self::Error>
    where
        Self: 'a;

    /// Bind to a local socket listening for incoming connections
    ///
    /// Depending on the platform, this method might actually be a no-op and just return a new acceptor
    /// implementation, that does the actual binding.
    /// Platforms that do not maintain internal acceptor queue (Embassy networking stack and `smoltcp`) are such examples.
    async fn bind(&self, local: SocketAddr) -> Result<Self::Accept<'_>, Self::Error>;
}

/// This is a factory trait for accepting incoming connections on server-side TCP sockets
pub trait TcpAccept {
    /// Error type returned on socket creation failure
    type Error: Error;

    /// The socket type returned by the factory
    type Socket<'a>: Read<Error = Self::Error> + Write<Error = Self::Error>
    where
        Self: 'a;

    /// Accepts an incoming connection
    /// Returns the socket address of the remote peer, as well as the accepted socket.
    async fn accept(&self) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error>;
}

impl<T> TcpConnect for &T
where
    T: TcpConnect,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        (*self).connect(remote).await
    }
}

impl<T> TcpConnect for &mut T
where
    T: TcpConnect,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        (**self).connect(remote).await
    }
}

impl<T> TcpBind for &T
where
    T: TcpBind,
{
    type Error = T::Error;

    type Accept<'a> = T::Accept<'a> where Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<Self::Accept<'_>, Self::Error> {
        (*self).bind(local).await
    }
}

impl<T> TcpBind for &mut T
where
    T: TcpBind,
{
    type Error = T::Error;

    type Accept<'a> = T::Accept<'a> where Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<Self::Accept<'_>, Self::Error> {
        (**self).bind(local).await
    }
}

impl<T> TcpAccept for &T
where
    T: TcpAccept,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn accept(&self) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        (*self).accept().await
    }
}

impl<T> TcpAccept for &mut T
where
    T: TcpAccept,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn accept(&self) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        (**self).accept().await
    }
}
