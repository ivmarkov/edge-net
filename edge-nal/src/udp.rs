//! Traits for modeling UDP sending/receiving functionality on embedded devices

use core::net::SocketAddr;

use embedded_io_async::ErrorType;

/// This trait is implemented by UDP sockets and models their datagram receiving functionality.
///
/// The socket it represents might be either bound (has a local IP address, port and interface) or
/// connected (also has a remote IP address and port).
///
/// The term "connected" here refers to the semantics of POSIX datagram sockets, through which datagrams
/// are sent and received without having a remote address per call. It does not imply any process
/// of establishing a connection (which is absent in UDP). While there is typically no POSIX
/// `bind()` call in the creation of such sockets, these are implicitly bound to a suitable local
/// address at connect time.
pub trait UdpReceive: ErrorType {
    /// Receive a datagram into the provided buffer.
    ///
    /// If the received datagram exceeds the buffer's length, it is received regardless, and the
    /// remaining bytes are discarded. The full datagram size is still indicated in the result,
    /// allowing the recipient to detect that truncation.
    ///
    /// The remote addresses is given in the result along with the number of bytes.
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, SocketAddr), Self::Error>;
}

/// This trait is implemented by UDP sockets and models their datagram sending functionality.
///
/// The socket it represents might be either bound (has a local IP address, port and interface) or
/// connected (also has a remote IP address and port).
///
/// The term "connected" here refers to the semantics of POSIX datagram sockets, through which datagrams
/// are sent and received without having a remote address per call. It does not imply any process
/// of establishing a connection (which is absent in UDP). While there is typically no POSIX
/// `bind()` call in the creation of such sockets, these are implicitly bound to a suitable local
/// address at connect time.
pub trait UdpSend: ErrorType {
    /// Send the provided data to a peer:
    /// - In case the socket is connected, the provided remote address is ignored.
    /// - In case the socket is unconnected the remote address is used.
    async fn send(&mut self, remote: SocketAddr, data: &[u8]) -> Result<(), Self::Error>;
}

pub trait UdpSocket: UdpReceive + UdpSend {}

impl<T> UdpReceive for &mut T
where
    T: UdpReceive,
{
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, SocketAddr), Self::Error> {
        (**self).receive(buffer).await
    }
}

impl<T> UdpSend for &mut T
where
    T: UdpSend,
{
    async fn send(&mut self, remote: SocketAddr, data: &[u8]) -> Result<(), Self::Error> {
        (**self).send(remote, data).await
    }
}
