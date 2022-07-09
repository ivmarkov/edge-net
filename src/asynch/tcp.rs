use core::future::Future;

use embedded_nal_async::SocketAddr;

/// This trait is implemented by TCP/IP stacks. In contrast to the TcpClientStack trait,
/// this trait allows creating a single connection at a time using the same TCP/IP stack.
///
/// The TCP/IP stack implementation can pre-create the sockets and hand them out.
pub trait TcpClientSocket:
    embedded_io::Io + embedded_io::asynch::Read + embedded_io::asynch::Write
{
    /// Future returned by `connect` function.
    type ConnectFuture<'m>: Future<Output = Result<(), Self::Error>> + 'm
    where
        Self: 'm;

    /// Connect to the given remote host and port.
    ///
    /// Returns `Ok` if the connection was successful.
    fn connect<'m>(&'m mut self, remote: SocketAddr) -> Self::ConnectFuture<'m>;

    /// Future returned by `is_connected` function.
    type IsConnectedFuture<'m>: Future<Output = Result<bool, Self::Error>> + 'm
    where
        Self: 'm;

    /// Check if this socket is connected
    fn is_connected<'m>(&'m mut self) -> Self::IsConnectedFuture<'m>;

    /// Disconnect from the remote host if connected.
    ///
    /// Returns `Ok` if the disconnection was successful.
    fn disconnect(&mut self) -> Result<(), Self::Error>;
}

impl<T> TcpClientSocket for &mut T
where
    T: TcpClientSocket,
{
    type ConnectFuture<'m>
    where
        Self: 'm,
    = T::ConnectFuture<'m>;

    fn connect<'m>(&'m mut self, remote: SocketAddr) -> Self::ConnectFuture<'m> {
        (*self).connect(remote)
    }

    type IsConnectedFuture<'m>
    where
        Self: 'm,
    = T::IsConnectedFuture<'m>;

    fn is_connected<'m>(&'m mut self) -> Self::IsConnectedFuture<'m> {
        (*self).is_connected()
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        (*self).disconnect()
    }
}
