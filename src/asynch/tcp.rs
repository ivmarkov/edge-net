use no_std_net::SocketAddr;

pub trait TcpSplittableConnection {
    type Error: embedded_io::Error;

    type Read<'a>: embedded_io::asynch::Read<Error = Self::Error>
    where
        Self: 'a;
    type Write<'a>: embedded_io::asynch::Write<Error = Self::Error>
    where
        Self: 'a;

    async fn split(&mut self) -> Result<(Self::Read<'_>, Self::Write<'_>), Self::Error>;
}

impl<'t, T> TcpSplittableConnection for &'t mut T
where
    T: TcpSplittableConnection + 't,
{
    type Error = T::Error;

    type Read<'a> = T::Read<'a> where Self: 'a;

    type Write<'a> = T::Write<'a> where Self: 'a;

    async fn split(&mut self) -> Result<(Self::Read<'_>, Self::Write<'_>), Self::Error> {
        (**self).split().await
    }
}

pub trait TcpListen {
    type Error: embedded_io::Error;

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
    type Error: embedded_io::Error;

    type Connection<'m>: embedded_io::asynch::Read<Error = Self::Error>
        + embedded_io::asynch::Write<Error = Self::Error>
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
