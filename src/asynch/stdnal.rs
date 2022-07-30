use std::net::{TcpListener, TcpStream};
use std::{io, net::ToSocketAddrs};

use core::future::{ready, Future};

use async_io::Async;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;
use no_std_net::SocketAddr;

use crate::asynch::tcp::TcpClientSocket;
use crate::close::Close;

use super::tcp::{TcpAcceptor, TcpServerSocket};

pub struct StdTcpClientSocket(Option<Async<TcpStream>>);

impl StdTcpClientSocket {
    pub const fn new() -> Self {
        Self(None)
    }
}

impl Io for StdTcpClientSocket {
    type Error = io::Error;
}

impl Close for StdTcpClientSocket {
    fn close(&mut self) {
        let _ = self.disconnect();
    }
}

impl TcpClientSocket for StdTcpClientSocket {
    type ConnectFuture<'m>
    = impl Future<Output = Result<(), Self::Error>> where Self: 'm;

    type IsConnectedFuture<'m>
    = impl Future<Output = Result<bool, Self::Error>> where Self: 'm;

    fn connect(&mut self, remote: SocketAddr) -> Self::ConnectFuture<'_> {
        async move {
            self.disconnect()?;

            self.0 = Some(Async::<TcpStream>::connect(to_std_addr(remote)?).await?);

            Ok(())
        }
    }

    fn is_connected(&mut self) -> Self::IsConnectedFuture<'_> {
        ready(Ok(self.0.is_some()))
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        self.0 = None;

        Ok(())
    }
}

impl Read for StdTcpClientSocket {
    type ReadFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
        async move {
            if let Some(socket) = self.0.as_mut() {
                socket.read(buf).await
            } else {
                Err(io::ErrorKind::NotConnected.into())
            }
        }
    }
}

impl Write for StdTcpClientSocket {
    type WriteFuture<'a>
    = impl Future<Output = Result<usize, Self::Error>> where Self: 'a;

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
        async move {
            if let Some(socket) = self.0.as_mut() {
                socket.write(buf).await
            } else {
                Err(io::ErrorKind::NotConnected.into())
            }
        }
    }

    type FlushFuture<'a>
    = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move {
            if let Some(socket) = self.0.as_mut() {
                socket.flush().await
            } else {
                Err(io::ErrorKind::NotConnected.into())
            }
        }
    }
}

pub struct StdTcpServerSocket;

impl StdTcpServerSocket {
    pub const fn new() -> Self {
        Self
    }
}

impl Io for StdTcpServerSocket {
    type Error = io::Error;
}

impl TcpServerSocket for StdTcpServerSocket {
    type Acceptor<'m>
    = StdTcpAcceptor where Self: 'm;

    type BindFuture<'m>
    = impl Future<Output = Result<Self::Acceptor<'m>, Self::Error>> + 'm where Self: 'm;

    fn bind(&mut self, remote: SocketAddr) -> Self::BindFuture<'_> {
        async move { Async::<TcpListener>::bind(to_std_addr(remote)?).map(StdTcpAcceptor) }
    }
}

pub struct StdTcpAcceptor(Async<TcpListener>);

impl TcpAcceptor for StdTcpAcceptor {
    type Error = io::Error;

    type Connection<'m> = StdTcpConnection;

    type AcceptFuture<'m>
    = impl Future<Output = Result<Self::Connection<'m>, Self::Error>> + 'm where Self: 'm;

    fn accept(&self) -> Self::AcceptFuture<'_> {
        async move {
            Ok(StdTcpConnection(
                self.0.accept().await.map(|(socket, _)| socket)?,
            ))
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
