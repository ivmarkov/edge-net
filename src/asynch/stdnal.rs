use std::io;
use std::net::{self, TcpStream, ToSocketAddrs, UdpSocket};

use async_io::Async;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use embedded_io::ErrorType;
use embedded_io_async::{Read, Write};
use no_std_net::{SocketAddr, SocketAddrV4, SocketAddrV6};

use embedded_nal_async::{
    AddrType, ConnectedUdp, Dns, IpAddr, TcpConnect, UdpStack, UnconnectedUdp,
};

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
        let connection = Async::<TcpStream>::connect(to_std_addr(remote)).await?;

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
        Async::<net::TcpListener>::bind(to_std_addr(remote)).map(StdTcpAccept)
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

pub struct StdUdpStack(());

impl StdUdpStack {
    pub const fn new() -> Self {
        Self(())
    }
}

impl UdpStack for StdUdpStack {
    type Error = io::Error;

    type Connected = StdUdpSocket;

    type UniquelyBound = StdUdpSocket;

    type MultiplyBound = StdUdpSocket;

    async fn connect_from(
        &self,
        local: SocketAddr,
        remote: SocketAddr,
    ) -> Result<(SocketAddr, Self::Connected), Self::Error> {
        let socket = Async::<UdpSocket>::bind(to_std_addr(local))?;

        socket.as_ref().connect(to_std_addr(remote))?;

        Ok((
            to_nal_addr(socket.as_ref().local_addr()?),
            StdUdpSocket(socket),
        ))
    }

    async fn bind_single(
        &self,
        local: SocketAddr,
    ) -> Result<(SocketAddr, Self::UniquelyBound), Self::Error> {
        let socket = Async::<UdpSocket>::bind(to_std_addr(local))?;

        Ok((
            to_nal_addr(socket.as_ref().local_addr()?),
            StdUdpSocket(socket),
        ))
    }

    async fn bind_multiple(&self, _local: SocketAddr) -> Result<Self::MultiplyBound, Self::Error> {
        unimplemented!()
    }
}

pub struct StdUdpSocket(Async<UdpSocket>);

impl ConnectedUdp for StdUdpSocket {
    type Error = io::Error;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let mut offset = 0;

        loop {
            offset += self.0.send(&data[offset..]).await?;

            if offset == 0 {
                break;
            }
        }

        Ok(())
    }

    async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.recv(buffer).await
    }
}

impl UnconnectedUdp for StdUdpSocket {
    type Error = io::Error;

    async fn send(
        &mut self,
        local: SocketAddr,
        remote: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        assert!(local == to_nal_addr(self.0.as_ref().local_addr()?));

        let mut offset = 0;

        loop {
            offset += self.0.send_to(data, to_std_addr(remote)).await?;

            if offset == 0 {
                break;
            }
        }

        Ok(())
    }

    async fn receive_into(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, SocketAddr, SocketAddr), Self::Error> {
        let (len, addr) = self.0.recv_from(buffer).await?;

        Ok((
            len,
            to_nal_addr(self.0.as_ref().local_addr()?),
            to_nal_addr(addr),
        ))
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

fn to_std_addr(addr: SocketAddr) -> std::net::SocketAddr {
    match addr {
        SocketAddr::V4(addr) => net::SocketAddr::V4(net::SocketAddrV4::new(
            addr.ip().octets().into(),
            addr.port(),
        )),
        SocketAddr::V6(addr) => net::SocketAddr::V6(net::SocketAddrV6::new(
            addr.ip().octets().into(),
            addr.port(),
            addr.flowinfo(),
            addr.scope_id(),
        )),
    }
}

fn to_nal_addr(addr: std::net::SocketAddr) -> SocketAddr {
    match addr {
        net::SocketAddr::V4(addr) => {
            SocketAddr::V4(SocketAddrV4::new(addr.ip().octets().into(), addr.port()))
        }
        net::SocketAddr::V6(addr) => SocketAddr::V6(SocketAddrV6::new(
            addr.ip().octets().into(),
            addr.port(),
            addr.flowinfo(),
            addr.scope_id(),
        )),
    }
}
