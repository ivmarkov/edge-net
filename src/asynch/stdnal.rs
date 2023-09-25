use std::io;
use std::net::{self, TcpStream, ToSocketAddrs};

use async_io::Async;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use embedded_io::ErrorType;
use embedded_io_async::{Read, Write};
use no_std_net::SocketAddr;

use embedded_nal_async::{AddrType, Dns, IpAddr, TcpConnect};

use super::tcp::{TcpAccept, TcpListen, TcpSplittableConnection};

pub struct StdTcpConnect(());

impl StdTcpConnect {
    pub const fn new() -> Self {
        Self(())
    }
}

impl TcpConnect for StdTcpConnect {
    type Error = io::Error;

    type Connection<'m> = StdTcpConnection where Self: 'm;

    async fn connect<'m>(&'m self, remote: SocketAddr) -> Result<Self::Connection<'m>, Self::Error>
    where
        Self: 'm,
    {
        let connection = Async::<TcpStream>::connect(to_std_addr(remote)?).await?;

        Ok(StdTcpConnection(connection))
    }
}

pub struct StdTcpListen(());

impl StdTcpListen {
    pub const fn new() -> Self {
        Self(())
    }
}

impl TcpListen for StdTcpListen {
    type Error = io::Error;

    type Acceptor<'m>
    = StdTcpAccept where Self: 'm;

    async fn listen(&self, remote: SocketAddr) -> Result<Self::Acceptor<'_>, Self::Error> {
        Async::<net::TcpListener>::bind(to_std_addr(remote)?).map(StdTcpAccept)
    }
}

pub struct StdTcpAccept(Async<net::TcpListener>);

impl TcpAccept for StdTcpAccept {
    type Error = io::Error;

    type Connection<'m> = StdTcpConnection;

    async fn accept(&self) -> Result<Self::Connection<'_>, Self::Error> {
        let connection = self.0.accept().await.map(|(socket, _)| socket)?;

        Ok(StdTcpConnection(connection))
    }
}

pub struct StdTcpConnection(Async<TcpStream>);

impl ErrorType for StdTcpConnection {
    type Error = io::Error;
}

impl Read for StdTcpConnection {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await
    }
}

impl Write for StdTcpConnection {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await
    }
}

impl TcpSplittableConnection for StdTcpConnection {
    type Error = io::Error;

    type Read<'a> = StdTcpConnectionRef<'a> where Self: 'a;

    type Write<'a> = StdTcpConnectionRef<'a> where Self: 'a;

    async fn split(&mut self) -> Result<(Self::Read<'_>, Self::Write<'_>), io::Error> {
        Ok((StdTcpConnectionRef(&self.0), StdTcpConnectionRef(&self.0)))
    }
}

pub struct StdTcpConnectionRef<'r>(&'r Async<TcpStream>);

impl<'r> ErrorType for StdTcpConnectionRef<'r> {
    type Error = io::Error;
}

impl<'r> Read for StdTcpConnectionRef<'r> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await
    }
}

impl<'r> Write for StdTcpConnectionRef<'r> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await
    }
}

pub struct StdDns<U>(U);

impl<U> StdDns<U> {
    pub const fn new(unblocker: U) -> Self {
        Self(unblocker)
    }
}

impl<U> Dns for StdDns<U>
where
    U: crate::asynch::Unblocker,
{
    type Error = io::Error;

    async fn get_host_by_name(
        &self,
        host: &str,
        addr_type: AddrType,
    ) -> Result<IpAddr, Self::Error> {
        let host = host.to_string();

        self.0
            .unblock(move || dns_lookup_host(&host, addr_type))
            .await
    }

    async fn get_host_by_address(
        &self,
        _addr: IpAddr,
    ) -> Result<heapless::String<256>, Self::Error> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

impl Dns for StdDns<()> {
    type Error = io::Error;

    async fn get_host_by_name(
        &self,
        host: &str,
        addr_type: AddrType,
    ) -> Result<IpAddr, Self::Error> {
        dns_lookup_host(host, addr_type)
    }

    async fn get_host_by_address(
        &self,
        _addr: IpAddr,
    ) -> Result<heapless::String<256>, Self::Error> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

fn dns_lookup_host(host: &str, addr_type: AddrType) -> Result<IpAddr, io::Error> {
    (host, 0_u16)
        .to_socket_addrs()?
        .find(|addr| match addr_type {
            AddrType::IPv4 => matches!(addr, std::net::SocketAddr::V4(_)),
            AddrType::IPv6 => matches!(addr, std::net::SocketAddr::V6(_)),
            AddrType::Either => true,
        })
        .map(|addr| match addr {
            std::net::SocketAddr::V4(v4) => v4.ip().octets().into(),
            std::net::SocketAddr::V6(v6) => v6.ip().octets().into(),
        })
        .ok_or_else(|| io::ErrorKind::AddrNotAvailable.into())
}

fn to_std_addr(addr: SocketAddr) -> std::io::Result<std::net::SocketAddr> {
    format!("{}:{}", addr.ip(), addr.port())
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::ErrorKind::AddrNotAvailable.into())
}
