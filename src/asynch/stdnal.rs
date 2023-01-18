use core::future::Future;

use std::io;
use std::net::{self, TcpStream, ToSocketAddrs};

use async_io::Async;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;
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

    type Connection<'m> = StdTcpConnection;

    type ConnectFuture<'m>
    = impl Future<Output = Result<StdTcpConnection, Self::Error>> + 'm where Self: 'm;

    fn connect(&self, remote: SocketAddr) -> Self::ConnectFuture<'_> {
        async move {
            let connection = Async::<TcpStream>::connect(to_std_addr(remote)?).await?;

            Ok(StdTcpConnection(connection))
        }
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

    type ListenFuture<'m>
    = impl Future<Output = Result<Self::Acceptor<'m>, Self::Error>> + 'm where Self: 'm;

    fn listen(&self, remote: SocketAddr) -> Self::ListenFuture<'_> {
        async move { Async::<net::TcpListener>::bind(to_std_addr(remote)?).map(StdTcpAccept) }
    }
}

pub struct StdTcpAccept(Async<net::TcpListener>);

impl TcpAccept for StdTcpAccept {
    type Error = io::Error;

    type Connection<'m> = StdTcpConnection;

    type AcceptFuture<'m>
    = impl Future<Output = Result<Self::Connection<'m>, Self::Error>> + 'm where Self: 'm;

    fn accept(&self) -> Self::AcceptFuture<'_> {
        async move {
            let connection = self.0.accept().await.map(|(socket, _)| socket)?;

            Ok(StdTcpConnection(connection))
        }
    }
}

pub struct StdTcpConnection(Async<TcpStream>);

impl Io for StdTcpConnection {
    type Error = io::Error;
}

impl Read for StdTcpConnection {
    type ReadFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> + 'a where Self: 'a;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move { self.0.read(buf).await }
    }
}

impl Write for StdTcpConnection {
    type WriteFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> + 'a where Self: 'a;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move { self.0.write(buf).await }
    }

    type FlushFuture<'a>
    = impl Future<Output = Result<(), Self::Error>> + 'a where Self: 'a;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move { self.0.flush().await }
    }
}

impl TcpSplittableConnection for StdTcpConnection {
    type Error = io::Error;

    type Read<'a> = StdTcpConnectionRef<'a> where Self: 'a;

    type Write<'a> = StdTcpConnectionRef<'a> where Self: 'a;

    type SplitFuture<'a> = impl Future<Output = Result<(Self::Read<'a>, Self::Write<'a>), io::Error>> where Self: 'a;

    fn split(&mut self) -> Self::SplitFuture<'_> {
        async move { Ok((StdTcpConnectionRef(&self.0), StdTcpConnectionRef(&self.0))) }
    }
}

pub struct StdTcpConnectionRef<'r>(&'r Async<TcpStream>);

impl<'r> Io for StdTcpConnectionRef<'r> {
    type Error = io::Error;
}

impl<'r> Read for StdTcpConnectionRef<'r> {
    type ReadFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> + 'a where Self: 'a;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move { self.0.read(buf).await }
    }
}

impl<'r> Write for StdTcpConnectionRef<'r> {
    type WriteFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> + 'a where Self: 'a;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move { self.0.write(buf).await }
    }

    type FlushFuture<'a>
    = impl Future<Output = Result<(), Self::Error>> + 'a where Self: 'a;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move { self.0.flush().await }
    }
}

pub struct StdDns<U>(U);

impl<U> StdDns<U>
where
    U: crate::asynch::Unblocker,
{
    pub const fn new(unblocker: U) -> Self {
        Self(unblocker)
    }
}

impl<U> Dns for StdDns<U>
where
    U: crate::asynch::Unblocker,
{
    type Error = io::Error;

    type GetHostByNameFuture<'m> = impl Future<Output = Result<IpAddr, Self::Error>> + 'm
	where Self: 'm;

    fn get_host_by_name<'m>(
        &'m self,
        host: &'m str,
        addr_type: AddrType,
    ) -> Self::GetHostByNameFuture<'m> {
        let host = host.to_string();

        async move {
            self.0
                .unblock(move || dns_lookup_host(&host, addr_type))
                .await
        }
    }

    type GetHostByAddressFuture<'m> = impl Future<Output = Result<heapless::String<256>, Self::Error>> + 'm
	where Self: 'm;

    fn get_host_by_address<'m>(&'m self, _addr: IpAddr) -> Self::GetHostByAddressFuture<'m> {
        async move { Err(io::ErrorKind::Unsupported.into()) }
    }
}

pub struct StdBlockingDns;

impl Dns for StdBlockingDns {
    type Error = io::Error;

    type GetHostByNameFuture<'m> = impl Future<Output = Result<IpAddr, Self::Error>> + 'm
	where Self: 'm;

    fn get_host_by_name<'m>(
        &'m self,
        host: &'m str,
        addr_type: AddrType,
    ) -> Self::GetHostByNameFuture<'m> {
        let host = host.to_string();

        async move { dns_lookup_host(&host, addr_type) }
    }

    type GetHostByAddressFuture<'m> = impl Future<Output = Result<heapless::String<256>, Self::Error>> + 'm
	where Self: 'm;

    fn get_host_by_address<'m>(&'m self, _addr: IpAddr) -> Self::GetHostByAddressFuture<'m> {
        async move { Err(io::ErrorKind::Unsupported.into()) }
    }
}

fn dns_lookup_host(host: &str, addr_type: AddrType) -> Result<IpAddr, io::Error> {
    (host, 0_u16)
        .to_socket_addrs()?
        .into_iter()
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
