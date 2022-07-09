use std::net::TcpStream;
use std::{io, net::ToSocketAddrs};

use core::future::{ready, Future};

use async_io::Async;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use embedded_io::asynch::{Read, Write};
use embedded_io::Io;

use crate::asynch::tcp::TcpClientSocket;

pub struct StdTcpClientSocket(Option<Async<TcpStream>>);

impl StdTcpClientSocket {
    pub const fn new() -> Self {
        Self(None)
    }
}

impl Io for StdTcpClientSocket {
    type Error = io::Error;
}

impl TcpClientSocket for StdTcpClientSocket {
    type ConnectFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<(), Self::Error>>;

    type IsConnectedFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<bool, Self::Error>>;

    fn connect(&mut self, remote: embedded_nal_async::SocketAddr) -> Self::ConnectFuture<'_> {
        async move {
            self.disconnect()?;

            self.0 = Some(
                Async::<TcpStream>::connect(
                    format!("{}:{}", remote.ip(), remote.port())
                        .to_socket_addrs()?
                        .next()
                        .unwrap(),
                )
                .await?,
            );

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
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

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
    where
        Self: 'a,
    = impl Future<Output = Result<usize, Self::Error>>;

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
    where
        Self: 'a,
    = impl Future<Output = Result<(), Self::Error>>;

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
