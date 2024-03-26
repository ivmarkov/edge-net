//! Factory traits for creating TCP sockets on embedded devices

use core::net::SocketAddr;

use embedded_io_async::{Error, ErrorType, Read, Write};

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

pub trait TcpConnect {
    type Error: Error;

    type Socket<'a>: Read<Error = Self::Error>
        + Write<Error = Self::Error>
        + TcpSplit<Error = Self::Error>
    where
        Self: 'a;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error>;
}

pub trait TcpBind {
    type Error: Error;

    type Accept<'a>: TcpAccept<Error = Self::Error>
    where
        Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<Self::Accept<'_>, Self::Error>;
}

pub trait TcpAccept {
    type Error: Error;

    type Socket<'a>: Read<Error = Self::Error>
        + Write<Error = Self::Error>
        + TcpSplit<Error = Self::Error>
    where
        Self: 'a;

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
