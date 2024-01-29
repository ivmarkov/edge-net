use embedded_io_async::ErrorType;
use embedded_nal_async::SocketAddr;

pub trait TcpSplittableConnection: ErrorType {
    type Read<'a>: embedded_io_async::Read<Error = Self::Error>
    where
        Self: 'a;
    type Write<'a>: embedded_io_async::Write<Error = Self::Error>
    where
        Self: 'a;

    fn split(&mut self) -> Result<(Self::Read<'_>, Self::Write<'_>), Self::Error>;
}

impl<'t, T> TcpSplittableConnection for &'t mut T
where
    T: TcpSplittableConnection + 't,
{
    type Read<'a> = T::Read<'a> where Self: 'a;

    type Write<'a> = T::Write<'a> where Self: 'a;

    fn split(&mut self) -> Result<(Self::Read<'_>, Self::Write<'_>), Self::Error> {
        (**self).split()
    }
}

pub trait TcpListen {
    type Error: embedded_io_async::Error;

    type Acceptor<'m>: TcpAccept<Error = Self::Error>
    where
        Self: 'm;

    async fn listen(&self, remote: SocketAddr) -> Result<Self::Acceptor<'_>, Self::Error>;
}

impl<T> TcpListen for &T
where
    T: TcpListen,
{
    type Error = T::Error;

    type Acceptor<'m> = T::Acceptor<'m>
    where Self: 'm;

    async fn listen(&self, remote: SocketAddr) -> Result<Self::Acceptor<'_>, Self::Error> {
        (*self).listen(remote).await
    }
}

impl<T> TcpListen for &mut T
where
    T: TcpListen,
{
    type Error = T::Error;

    type Acceptor<'m> = T::Acceptor<'m>
    where Self: 'm;

    async fn listen(&self, remote: SocketAddr) -> Result<Self::Acceptor<'_>, Self::Error> {
        (**self).listen(remote).await
    }
}

pub trait TcpAccept {
    type Error: embedded_io_async::Error;
    
    type Connection<'m>: embedded_io_async::Read<Error = Self::Error>
        + embedded_io_async::Write<Error = Self::Error>
    where
        Self: 'm;

    async fn accept(&self) -> Result<Self::Connection<'_>, Self::Error>;
}

impl<T> TcpAccept for &T
where
    T: TcpAccept,
{
    type Error = T::Error;

    type Connection<'m> = T::Connection<'m>
    where Self: 'm;

    async fn accept(&self) -> Result<Self::Connection<'_>, Self::Error> {
        (**self).accept().await
    }
}

impl<T> TcpAccept for &mut T
where
    T: TcpAccept,
{
    type Error = T::Error;

    type Connection<'m> = T::Connection<'m>
    where Self: 'm;

    async fn accept(&self) -> Result<Self::Connection<'_>, Self::Error> {
        (**self).accept().await
    }
}
