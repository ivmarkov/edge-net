//! Trait for modeling TCP socket shutdown

use embedded_io_async::ErrorType;

/// Enum representing the different ways to close a TCP socket
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Close {
    /// Close the read half of the socket
    Read,
    /// Close the write half of the socket
    Write,
    /// Close both the read and write halves of the socket
    Both,
}

/// This trait is implemented by TCP sockets and models their shutdown functionality,
/// which is unique to the TCP protocol (UDP sockets do not have a shutdown procedure).
pub trait TcpShutdown: ErrorType {
    /// Gracefully shutdown either or both the read and write halves of the socket.
    ///
    /// The write half is closed by sending a FIN packet to the peer and then waiting
    /// until the FIN packet is ACKed.
    ///
    /// The read half is "closed" by reading from it until the peer indicates there is
    /// no more data to read (i.e. it sends a FIN packet to the local socket).
    /// Whether the other peer will send a FIN packet or not is not guaranteed, as that's
    /// application protocol-specific. Usually, closing the write half means the peer will
    /// notice and will send a FIN packet to the read half, thus "closing" it too.
    ///
    /// Note that on certain platforms that don't have timeouts this method might never
    /// complete if the peer is unreachable / misbehaving, so it has to be used with a
    /// proper timeout in-place.
    ///
    /// Also note that calling this function multiple times may result in different behavior,
    /// depending on the platform.
    async fn close(&mut self, what: Close) -> Result<(), Self::Error>;

    /// Abort the connection, sending an RST packet to the peer
    ///
    /// This method will not wait forever, because the RST packet is not ACKed by the peer.
    ///
    /// Note that on certain platforms (STD for example) this method might be a no-op
    /// as the connection there is automatically aborted when the socket is dropped.
    ///
    /// Also note that calling this function multiple times may result in different behavior,
    /// depending on the platform.
    async fn abort(&mut self) -> Result<(), Self::Error>;
}

impl<T> TcpShutdown for &mut T
where
    T: TcpShutdown,
{
    async fn close(&mut self, what: Close) -> Result<(), Self::Error> {
        (**self).close(what).await
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        (**self).abort().await
    }
}
