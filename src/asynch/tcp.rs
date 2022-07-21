use core::future::Future;

use no_std_net::SocketAddr;

use crate::close::Close;

/// This trait is implemented by TCP/IP stacks. In contrast to the TcpClientStack trait,
/// this trait allows creating a single connection at a time using the same TCP/IP stack.
///
/// The TCP/IP stack implementation can pre-create the sockets and hand them out.
pub trait TcpClientSocket:
    embedded_io::Io + embedded_io::asynch::Read + embedded_io::asynch::Write + Close
{
    /// Future returned by `connect` function.
    type ConnectFuture<'m>: Future<Output = Result<(), Self::Error>> + 'm
    where
        Self: 'm;

    /// Connect to the given remote host and port.
    ///
    /// Returns `Ok` if the connection was successful.
    fn connect(&mut self, remote: SocketAddr) -> Self::ConnectFuture<'_>;

    /// Future returned by `is_connected` function.
    type IsConnectedFuture<'m>: Future<Output = Result<bool, Self::Error>> + 'm
    where
        Self: 'm;

    /// Check if this socket is connected
    fn is_connected(&mut self) -> Self::IsConnectedFuture<'_>;

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

    fn connect(&mut self, remote: SocketAddr) -> Self::ConnectFuture<'_> {
        (*self).connect(remote)
    }

    type IsConnectedFuture<'m>
    where
        Self: 'm,
    = T::IsConnectedFuture<'m>;

    fn is_connected(&mut self) -> Self::IsConnectedFuture<'_> {
        (*self).is_connected()
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        (*self).disconnect()
    }
}

pub trait TcpServerSocket: embedded_io::Io {
    type Acceptor<'m>: TcpAcceptor<Error = Self::Error>
    where
        Self: 'm;

    type BindFuture<'m>: Future<Output = Result<Self::Acceptor<'m>, Self::Error>> + 'm
    where
        Self: 'm;

    fn bind(&mut self, remote: SocketAddr) -> Self::BindFuture<'_>;
}

pub trait TcpAcceptor {
    type Error: embedded_io::Error;

    type Connection<'m>: embedded_io::asynch::Read<Error = Self::Error>
        + embedded_io::asynch::Write<Error = Self::Error>
    where
        Self: 'm;

    type AcceptFuture<'m>: Future<Output = Result<Self::Connection<'m>, Self::Error>> + 'm
    where
        Self: 'm;

    fn accept(&self) -> Self::AcceptFuture<'_>;
}

impl<T> TcpServerSocket for &mut T
where
    T: TcpServerSocket,
{
    type Acceptor<'m>
    where
        Self: 'm,
    = T::Acceptor<'m>;

    type BindFuture<'m>
    where
        Self: 'm,
    = T::BindFuture<'m>;

    fn bind(&mut self, remote: SocketAddr) -> Self::BindFuture<'_> {
        (*self).bind(remote)
    }
}

impl<T> TcpAcceptor for &T
where
    T: TcpAcceptor,
{
    type Error = T::Error;

    type Connection<'m>
    where
        Self: 'm,
    = T::Connection<'m>;

    type AcceptFuture<'m>
    where
        Self: 'm,
    = T::AcceptFuture<'m>;

    fn accept(&self) -> Self::AcceptFuture<'_> {
        (**self).accept()
    }
}

impl<T> TcpAcceptor for &mut T
where
    T: TcpAcceptor,
{
    type Error = T::Error;

    type Connection<'m>
    where
        Self: 'm,
    = T::Connection<'m>;

    type AcceptFuture<'m>
    where
        Self: 'm,
    = T::AcceptFuture<'m>;

    fn accept(&self) -> Self::AcceptFuture<'_> {
        (**self).accept()
    }
}
