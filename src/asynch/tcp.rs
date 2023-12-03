use core::fmt::Debug;

use no_std_net::SocketAddr;

pub trait TcpSplittableConnection {
    type Error: embedded_io::Error;

    type Read<'a>: embedded_io_async::Read<Error = Self::Error>
    where
        Self: 'a;
    type Write<'a>: embedded_io_async::Write<Error = Self::Error>
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

// TODO: Ideally should go to `embedded-nal-async`
pub trait RawSocket {
    type Error: Debug + embedded_io_async::Error;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
}

// TODO: Ideally should go to `embedded-nal-async`
impl<T> RawSocket for &mut T
where
    T: RawSocket,
{
    type Error = T::Error;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        (**self).send(data).await
    }

    async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        (**self).receive_into(buffer).await
    }
}

// TODO: Ideally should go to `embedded-nal-async`
pub trait RawStack {
    type Error: Debug;

    type Socket: RawSocket<Error = Self::Error>;

    async fn connect(&self, interface: Option<u32>) -> Result<Self::Socket, Self::Error>;
}

// TODO: Ideally should go to `embedded-nal-async`
impl<T> RawStack for &T
where
    T: RawStack,
{
    type Error = T::Error;

    type Socket = T::Socket;

    async fn connect(&self, interface: Option<u32>) -> Result<Self::Socket, Self::Error> {
        (*self).connect(interface).await
    }
}

impl<T> RawStack for &mut T
where
    T: RawStack,
{
    type Error = T::Error;

    type Socket = T::Socket;

    async fn connect(&self, interface: Option<u32>) -> Result<Self::Socket, Self::Error> {
        (**self).connect(interface).await
    }
}

// pub struct IO<T>(pub T);

// impl<T> ErrorType for IO<T>
// where
//     T: RawSocket,
// {
//     type Error = T::Error;
// }

// impl<T> Read for IO<T>
// where
//     T: RawSocket,
// {
//     async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
//         self.0.receive_into(buf).await
//     }
// }

// impl<T> Write for IO<T>
// where
//     T: RawSocket,
// {
//     async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
//         self.0.send(buf).await?;

//         Ok(buf.len())
//     }
// }
