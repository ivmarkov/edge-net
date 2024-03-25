//! Traits for modeling raw sockets' sending/receiving functionality on embedded devices

use embedded_io_async::ErrorType;

/// A MAC address
pub type MacAddr = [u8; 6];

/// This trait is implemented by raw sockets and models their datagram receiving functionality.
pub trait RawReceive: ErrorType {
    /// Receive a datagram into the provided buffer.
    ///
    /// If the received datagram exceeds the buffer's length, it is received regardless, and the
    /// remaining bytes are discarded. The full datagram size is still indicated in the result,
    /// allowing the recipient to detect that truncation.
    ///
    /// The remote Mac address is given in the result along with the number
    /// of bytes.
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, MacAddr), Self::Error>;
}

/// This trait is implemented by UDP sockets and models their datagram sending functionality.
pub trait RawSend: ErrorType {
    /// Send the provided data to a peer.
    ///
    /// A MAC address is provided to specify the destination.
    /// If the destination mac address contains all `0xff`, the packet is broadcasted.
    async fn send(&mut self, addr: MacAddr, data: &[u8]) -> Result<(), Self::Error>;
}

impl<T> RawReceive for &mut T
where
    T: RawReceive,
{
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, MacAddr), Self::Error> {
        (**self).receive(buffer).await
    }
}

impl<T> RawSend for &mut T
where
    T: RawSend,
{
    async fn send(&mut self, addr: MacAddr, data: &[u8]) -> Result<(), Self::Error> {
        (**self).send(addr, data).await
    }
}
