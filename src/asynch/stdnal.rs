use std::io;

use core::future::Future;

use embedded_nal_async::TcpClientStack;

pub struct StdTcpClientStack {}

impl TcpClientStack for StdTcpClientStack {
    type TcpSocket = StdTcpSocket;

    type Error = io::Error;

    type SocketFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<Self::TcpSocket, Self::Error>> + 'm;

    fn socket<'m>(&'m mut self) -> Self::SocketFuture<'m> {
        async move { todo!() }
    }

    type ConnectFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<(), Self::Error>> + 'm;

    fn connect<'m>(
        &'m mut self,
        socket: &'m mut Self::TcpSocket,
        remote: embedded_nal_async::SocketAddr,
    ) -> Self::ConnectFuture<'m> {
        async move { todo!() }
    }

    type IsConnectedFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<bool, Self::Error>> + 'm;

    fn is_connected<'m>(&'m mut self, socket: &'m Self::TcpSocket) -> Self::IsConnectedFuture<'m> {
        async move { todo!() }
    }

    type SendFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<usize, Self::Error>> + 'm;

    fn send<'m>(
        &'m mut self,
        socket: &'m mut Self::TcpSocket,
        buffer: &'m [u8],
    ) -> Self::SendFuture<'m> {
        async move { todo!() }
    }

    type ReceiveFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<usize, Self::Error>> + 'm;

    fn receive<'m>(
        &'m mut self,
        socket: &'m mut Self::TcpSocket,
        buffer: &'m mut [u8],
    ) -> Self::ReceiveFuture<'m> {
        async move { todo!() }
    }

    type CloseFuture<'m>
    where
        Self: 'm,
    = impl Future<Output = Result<(), Self::Error>> + 'm;

    fn close<'m>(&'m mut self, socket: Self::TcpSocket) -> Self::CloseFuture<'m> {
        async move { todo!() }
    }
}

pub struct StdTcpSocket {}
