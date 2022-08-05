use std::net::{self, TcpStream};
use std::{io, net::ToSocketAddrs};

use core::future::Future;

use async_io::Async;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;
use no_std_net::SocketAddr;

use embedded_nal_async::TcpConnect;

use super::tcp::{TcpAccept, TcpListen};

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
    = impl Future<Output = Result<StdTcpConnection, Self::Error>> where Self: 'm;

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
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move { self.0.read(buf).await }
    }
}

impl Write for StdTcpConnection {
    type WriteFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move { self.0.write(buf).await }
    }

    type FlushFuture<'a>
    = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move { self.0.flush().await }
    }
}

fn to_std_addr(addr: SocketAddr) -> std::io::Result<std::net::SocketAddr> {
    format!("{}:{}", addr.ip(), addr.port())
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::ErrorKind::AddrNotAvailable.into())
}
