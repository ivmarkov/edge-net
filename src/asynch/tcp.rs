use core::future::Future;

use no_std_net::SocketAddr;

pub trait TcpSplittableConnection {
    type Error: embedded_io::Error;

    type Read<'a>: embedded_io::asynch::Read<Error = Self::Error>
    where
        Self: 'a;
    type Write<'a>: embedded_io::asynch::Write<Error = Self::Error>
    where
        Self: 'a;

    type SplitFuture<'a>: Future<Output = Result<(Self::Read<'a>, Self::Write<'a>), Self::Error>>
    where
        Self: 'a;

    fn split(&mut self) -> Self::SplitFuture<'_>;
}

pub trait TcpListen {
    type Error: embedded_io::Error;

    type Acceptor<'m>: TcpAccept<Error = Self::Error>
    where
        Self: 'm;

    type ListenFuture<'m>: Future<Output = Result<Self::Acceptor<'m>, Self::Error>> + 'm
    where
        Self: 'm;

    fn listen(&self, remote: SocketAddr) -> Self::ListenFuture<'_>;
}

pub trait TcpAccept {
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

impl<T> TcpListen for &T
where
    T: TcpListen,
{
    type Error = T::Error;

    type Acceptor<'m> = T::Acceptor<'m>
    where Self: 'm;

    type ListenFuture<'m> = T::ListenFuture<'m>
    where Self: 'm;

    fn listen(&self, remote: SocketAddr) -> Self::ListenFuture<'_> {
        (*self).listen(remote)
    }
}

impl<T> TcpAccept for &T
where
    T: TcpAccept,
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

impl<T> TcpAccept for &mut T
where
    T: TcpAccept,
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
