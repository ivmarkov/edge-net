use core::future::Future;

use no_std_net::SocketAddr;

pub trait TcpClientSocket {
    type Error: embedded_io::Error;

    /// Type holding state of a TCP connection.
    type Connection<'m>: embedded_io::asynch::Read<Error = Self::Error>
        + embedded_io::asynch::Write<Error = Self::Error>
    where
        Self: 'm;
    /// Future returned by `connect` function.
    type ConnectFuture<'m>: Future<Output = Result<Self::Connection<'m>, Self::Error>> + 'm
    where
        Self: 'm;

    /// Connect to the given remote host and port.
    ///
    /// Returns `Ok` if the connection was successful.
    fn connect<'m>(&'m self, remote: SocketAddr) -> Self::ConnectFuture<'m>;
}

impl<T> TcpClientSocket for &T
where
    T: TcpClientSocket,
{
    type Error = T::Error;

    type Connection<'m> = T::Connection<'m>
	where
		Self: 'm;

    type ConnectFuture<'m> = T::ConnectFuture<'m>
	where
		Self: 'm;

    fn connect<'m>(&'m self, remote: SocketAddr) -> Self::ConnectFuture<'m> {
        (*self).connect(remote)
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
    type Acceptor<'m> = T::Acceptor<'m>
    where Self: 'm;

    type BindFuture<'m> = T::BindFuture<'m>
    where Self: 'm;

    fn bind(&mut self, remote: SocketAddr) -> Self::BindFuture<'_> {
        (*self).bind(remote)
    }
}

impl<T> TcpAcceptor for &T
where
    T: TcpAcceptor,
{
    type Error = T::Error;

    type Connection<'m> = T::Connection<'m>
    where Self: 'm;

    type AcceptFuture<'m> = T::AcceptFuture<'m>
    where Self: 'm;

    fn accept(&self) -> Self::AcceptFuture<'_> {
        (**self).accept()
    }
}

impl<T> TcpAcceptor for &mut T
where
    T: TcpAcceptor,
{
    type Error = T::Error;

    type Connection<'m> = T::Connection<'m>
    where Self: 'm;

    type AcceptFuture<'m> = T::AcceptFuture<'m>
    where Self: 'm;

    fn accept(&self) -> Self::AcceptFuture<'_> {
        (**self).accept()
    }
}
