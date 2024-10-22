use core::net::SocketAddr;
use core::pin::pin;
use core::ptr::NonNull;

use edge_nal::{Close, Readable, TcpBind, TcpConnect, TcpShutdown, TcpSplit};

use embassy_futures::join::join;

use embassy_net::driver::Driver;
use embassy_net::tcp::{AcceptError, ConnectError, Error, TcpReader, TcpWriter};
use embassy_net::Stack;

use embedded_io_async::{ErrorKind, ErrorType, Read, Write};

use crate::{to_emb_bind_socket, to_emb_socket, to_net_socket, Pool};

/// A struct that implements the `TcpConnect` and `TcpBind` factory traits from `edge-nal`
/// Capable of managing up to N concurrent connections with TX and RX buffers according to TX_SZ and RX_SZ.
pub struct Tcp<'d, const N: usize, const TX_SZ: usize = 1024, const RX_SZ: usize = 1024> {
    stack: Stack<'d>,
    buffers: &'d TcpBuffers<N, TX_SZ, RX_SZ>,
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> Tcp<'d, N, TX_SZ, RX_SZ> {
    /// Create a new `Tcp` instance for the provided Embassy networking stack, using the provided TCP buffers
    ///
    /// Ensure that the number of buffers `N` fits within StackResources<N> of
    /// [embassy_net::Stack], while taking into account the sockets used for DHCP, DNS, etc. else
    /// [smoltcp::iface::SocketSet] will panic with `adding a socket to a full SocketSet`.
    pub fn new(stack: Stack<'d>, buffers: &'d TcpBuffers<N, TX_SZ, RX_SZ>) -> Self {
        Self { stack, buffers }
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpConnect
    for Tcp<'d, N, TX_SZ, RX_SZ>
{
    type Error = TcpError;

    type Socket<'a>
        = TcpSocket<'a, N, TX_SZ, RX_SZ>
    where
        Self: 'a;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        let mut socket = TcpSocket::new(self.stack, self.buffers)?;

        socket.socket.connect(to_emb_socket(remote)).await?;

        Ok(socket)
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpBind
    for Tcp<'d, N, TX_SZ, RX_SZ>
{
    type Error = TcpError;

    type Accept<'a>
        = TcpAccept<'a, N, TX_SZ, RX_SZ>
    where
        Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<Self::Accept<'_>, Self::Error> {
        Ok(TcpAccept { stack: self, local })
    }
}

/// Represents an acceptor for incoming TCP client connections. Implements the `TcpAccept` factory trait from `edge-nal`
pub struct TcpAccept<'d, const N: usize, const TX_SZ: usize = 1024, const RX_SZ: usize = 1024> {
    stack: &'d Tcp<'d, N, TX_SZ, RX_SZ>,
    local: SocketAddr,
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> edge_nal::TcpAccept
    for TcpAccept<'d, N, TX_SZ, RX_SZ>
{
    type Error = TcpError;

    type Socket<'a>
        = TcpSocket<'a, N, TX_SZ, RX_SZ>
    where
        Self: 'a;

    async fn accept(&self) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        let mut socket = TcpSocket::new(self.stack.stack, self.stack.buffers)?;

        socket.socket.accept(to_emb_bind_socket(self.local)).await?;

        let local_endpoint = socket.socket.local_endpoint().unwrap();

        Ok((to_net_socket(local_endpoint), socket))
    }
}

/// A TCP socket
/// Implements the `Read` and `Write` traits from `embedded-io-async`, as well as the `TcpSplit` factory trait from `edge-nal`
pub struct TcpSocket<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> {
    socket: embassy_net::tcp::TcpSocket<'d>,
    stack_buffers: &'d TcpBuffers<N, TX_SZ, RX_SZ>,
    socket_buffers: NonNull<([u8; TX_SZ], [u8; RX_SZ])>,
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpSocket<'d, N, TX_SZ, RX_SZ> {
    fn new(
        stack: Stack<'d>,
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

    async fn close(&mut self, what: Close) -> Result<(), TcpError> {
        async fn discard_all_data(rx: &mut TcpReader<'_>) -> Result<(), TcpError> {
            let mut buf = [0; 32];

            while rx.read(&mut buf).await? > 0 {}

            Ok(())
        }

        if matches!(what, Close::Both | Close::Write) {
            self.socket.close();
        }

        let (mut rx, mut tx) = self.socket.split();

        match what {
            Close::Read => discard_all_data(&mut rx).await?,
            Close::Write => tx.flush().await?,
            Close::Both => {
                let mut flush = pin!(tx.flush());
                let mut read = pin!(discard_all_data(&mut rx));

                match join(&mut flush, &mut read).await {
                    (Err(e), _) => Err(e)?,
                    (_, Err(e)) => Err(e)?,
                    _ => (),
                }
            }
        }

        Ok(())
    }

    async fn abort(&mut self) -> Result<(), TcpError> {
        self.socket.abort();
        self.socket.flush().await?;

        Ok(())
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

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> Readable
    for TcpSocket<'d, N, TX_SZ, RX_SZ>
{
    async fn readable(&mut self) -> Result<(), Self::Error> {
        panic!("Not implemented yet")
    }
}

impl<'d, const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpShutdown
    for TcpSocket<'d, N, TX_SZ, RX_SZ>
{
    async fn close(&mut self, what: Close) -> Result<(), Self::Error> {
        TcpSocket::close(self, what).await
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        TcpSocket::abort(self).await
    }
}

/// Represents the read half of a split TCP socket
/// Implements the `Read` trait from `embedded-io-async`
pub struct TcpSocketRead<'a>(TcpReader<'a>);

impl<'a> ErrorType for TcpSocketRead<'a> {
    type Error = TcpError;
}

impl<'a> Read for TcpSocketRead<'a> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await.map_err(TcpError::from)
    }
}

impl<'a> Readable for TcpSocketRead<'a> {
    async fn readable(&mut self) -> Result<(), Self::Error> {
        panic!("Not implemented yet")
    }
}

/// Represents the write half of a split TCP socket
/// Implements the `Write` trait from `embedded-io-async`
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
    type Read<'a>
        = TcpSocketRead<'a>
    where
        Self: 'a;

    type Write<'a>
        = TcpSocketWrite<'a>
    where
        Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        let (read, write) = self.socket.split();

        (TcpSocketRead(read), TcpSocketWrite(write))
    }
}

/// A shared error type that is used by the TCP factory traits implementation as well as the TCP socket
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

/// A struct that holds a pool of TCP buffers
pub struct TcpBuffers<const N: usize, const TX_SZ: usize, const RX_SZ: usize> {
    pool: Pool<([u8; TX_SZ], [u8; RX_SZ]), N>,
}

impl<const N: usize, const TX_SZ: usize, const RX_SZ: usize> TcpBuffers<N, TX_SZ, RX_SZ> {
    /// Create a new `TcpBuffers` instance
    pub const fn new() -> Self {
        Self { pool: Pool::new() }
    }
}
