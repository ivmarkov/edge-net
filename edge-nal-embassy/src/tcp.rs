use core::net::SocketAddr;
use core::ptr::NonNull;

use edge_nal::{TcpBind, TcpConnect, TcpSplit};

use embassy_net::driver::Driver;
use embassy_net::tcp::{AcceptError, ConnectError, Error, TcpReader, TcpWriter};
use embassy_net::Stack;

use embedded_io_async::{ErrorKind, ErrorType, Read, Write};

use crate::{to_emb_socket, to_net_socket, Pool};

/// TCP stack compatible with the `edge-nal` traits.
///
/// The stack is capable of managing up to N concurrent connections with tx and rx buffers according to TX_SZ and RX_SZ.
pub struct TcpStack<
    'd,
    D: Driver,
    const N: usize,
    const TX_SZ: usize = 1024,
    const RX_SZ: usize = 1024,
> {
    stack: &'d Stack<D>,
    buffers: &'d TcpBuffers<N, TX_SZ, RX_SZ>,
}

impl<'d, D: Driver, const N: usize, const TX_SZ: usize, const RX_SZ: usize>
    TcpStack<'d, D, N, TX_SZ, RX_SZ>
{
    /// Create a new `TcpStack`.
    pub fn new(stack: &'d Stack<D>, buffers: &'d TcpBuffers<N, TX_SZ, RX_SZ>) -> Self {
        Self { stack, buffers }
    }
}

impl<'d, D: Driver, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpConnect
    for TcpStack<'d, D, N, TX_SZ, RX_SZ>
{
    type Error = TcpError;

    type Socket<'a> = TcpSocket<'a, N, TX_SZ, RX_SZ> where Self: 'a;

    async fn connect(
        &self,
        remote: SocketAddr,
    ) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        let mut socket = TcpSocket::new(&self.stack, self.buffers)?;

        socket.socket.connect(to_emb_socket(remote)).await?;

        let local_endpoint = socket.socket.local_endpoint().unwrap();

        Ok((to_net_socket(local_endpoint), socket))
    }
}

impl<'d, D: Driver, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpBind
    for TcpStack<'d, D, N, TX_SZ, RX_SZ>
{
    type Error = TcpError;

    type Accept<'a> = TcpAccept<'a, D, N, TX_SZ, RX_SZ> where Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<(SocketAddr, Self::Accept<'_>), Self::Error> {
        Ok((local, TcpAccept { stack: self, local }))
    }
}

pub struct TcpAccept<
    'd,
    D: Driver,
    const N: usize,
    const TX_SZ: usize = 1024,
    const RX_SZ: usize = 1024,
> {
    stack: &'d TcpStack<'d, D, N, TX_SZ, RX_SZ>,
    local: SocketAddr,
}

impl<'d, D: Driver, const N: usize, const TX_SZ: usize, const RX_SZ: usize> edge_nal::TcpAccept
    for TcpAccept<'d, D, N, TX_SZ, RX_SZ>
{
    type Error = TcpError;

    type Socket<'a> = TcpSocket<'a, N, TX_SZ, RX_SZ> where Self: 'a;

    async fn accept(&self) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        let mut socket = TcpSocket::new(self.stack.stack, self.stack.buffers)?;

        socket.socket.accept(to_emb_socket(self.local)).await?;

        let local_endpoint = socket.socket.local_endpoint().unwrap();

        Ok((to_net_socket(local_endpoint), socket))
    }
}

/// A TCP socket
pub struct TcpSocket<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> {
    socket: embassy_net::tcp::TcpSocket<'d>,
    stack_buffers: &'d TcpBuffers<N, TX_SZ, RX_SZ>,
    socket_buffers: NonNull<([u8; TX_SZ], [u8; RX_SZ])>,
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpSocket<'d, N, TX_SZ, RX_SZ> {
    fn new<D: Driver>(
        stack: &'d Stack<D>,
        stack_buffers: &'d TcpBuffers<N, TX_SZ, RX_SZ>,
    ) -> Result<Self, TcpError> {
        let mut socket_buffers = stack_buffers.pool.alloc().ok_or(TcpError::NoBuffers)?;

        Ok(Self {
            socket: unsafe {
                embassy_net::tcp::TcpSocket::new(
                    stack,
                    &mut socket_buffers.as_mut().1,
                    &mut socket_buffers.as_mut().0,
                )
            },
            stack_buffers,
            socket_buffers,
        })
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> Drop
    for TcpSocket<'d, N, TX_SZ, RX_SZ>
{
    fn drop(&mut self) {
        unsafe {
            self.socket.close();
            self.stack_buffers.pool.free(self.socket_buffers);
        }
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> ErrorType
    for TcpSocket<'d, N, TX_SZ, RX_SZ>
{
    type Error = TcpError;
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> Read
    for TcpSocket<'d, N, TX_SZ, RX_SZ>
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(self.socket.read(buf).await?)
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> Write
    for TcpSocket<'d, N, TX_SZ, RX_SZ>
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        Ok(self.socket.write(buf).await?)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.socket.flush().await?;

        Ok(())
    }
}

pub struct TcpSocketRead<'a>(TcpReader<'a>);

impl<'a> ErrorType for TcpSocketRead<'a> {
    type Error = TcpError;
}

impl<'a> Read for TcpSocketRead<'a> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await.map_err(TcpError::from)
    }
}

pub struct TcpSocketWrite<'a>(TcpWriter<'a>);

impl<'a> ErrorType for TcpSocketWrite<'a> {
    type Error = TcpError;
}

impl<'a> Write for TcpSocketWrite<'a> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await.map_err(TcpError::from)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await.map_err(TcpError::from)
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpSplit
    for TcpSocket<'d, N, TX_SZ, RX_SZ>
{
    type Read<'a> = TcpSocketRead<'a> where Self: 'a;

    type Write<'a> = TcpSocketWrite<'a> where Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        let (read, write) = self.socket.split();

        (TcpSocketRead(read), TcpSocketWrite(write))
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TcpError {
    General(Error),
    Connect(ConnectError),
    Accept(AcceptError),
    NoBuffers,
}

impl From<Error> for TcpError {
    fn from(e: Error) -> Self {
        TcpError::General(e)
    }
}

impl From<ConnectError> for TcpError {
    fn from(e: ConnectError) -> Self {
        TcpError::Connect(e)
    }
}

impl From<AcceptError> for TcpError {
    fn from(e: AcceptError) -> Self {
        TcpError::Accept(e)
    }
}

// TODO
impl embedded_io_async::Error for TcpError {
    fn kind(&self) -> ErrorKind {
        match self {
            TcpError::General(_) => ErrorKind::Other,
            TcpError::Connect(_) => ErrorKind::Other,
            TcpError::Accept(_) => ErrorKind::Other,
            TcpError::NoBuffers => ErrorKind::OutOfMemory,
        }
    }
}

pub struct TcpBuffers<const N: usize, const TX_SZ: usize, const RX_SZ: usize> {
    pool: Pool<([u8; TX_SZ], [u8; RX_SZ]), N>,
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpBuffers<N, TX_SZ, RX_SZ> {
    pub const fn new() -> Self {
        Self { pool: Pool::new() }
    }
}
