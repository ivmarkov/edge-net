use core::fmt::{self, Debug};

use embedded_nal_async::{SocketAddr, SocketAddrV4, UdpStack, UnconnectedUdp};
use embedded_nal_async_xtra::UnconnectedUdpWithMac;

use crate as dhcp;

pub mod client;
pub mod server;

pub const DEFAULT_SERVER_PORT: u16 = 67;
pub const DEFAULT_CLIENT_PORT: u16 = 68;

#[derive(Debug)]
pub enum Error<E> {
    Io(E),
    Format(dhcp::Error),
}

impl<E> From<dhcp::Error> for Error<E> {
    fn from(value: dhcp::Error) -> Self {
        Self::Format(value)
    }
}

impl<E> fmt::Display for Error<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "IO error: {err}"),
            Self::Format(err) => write!(f, "Format error: {err}"),
        }
    }
}

#[cfg(feature = "std")]
impl<E> std::error::Error for Error<E> where E: std::error::Error {}

/// A fallback implementation of `UnconnectedUdpWithMac` that does not support MAC addresses.
/// Might or might not work depending on the DHCP client.
pub struct UnconnectedUdpWithMacFallback<T>(pub T);

impl<T> UnconnectedUdp for UnconnectedUdpWithMacFallback<T>
where
    T: UnconnectedUdp,
{
    type Error = T::Error;

    async fn send(
        &mut self,
        local: SocketAddr,
        remote: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.0.send(local, remote, data).await
    }

    async fn receive_into(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, SocketAddr, SocketAddr), Self::Error> {
        self.0.receive_into(buffer).await
    }
}

impl<T> UnconnectedUdpWithMac for UnconnectedUdpWithMacFallback<T>
where
    T: UnconnectedUdp,
{
    async fn send(
        &mut self,
        local: SocketAddr,
        remote: SocketAddr,
        _remote_mac: Option<&[u8; 6]>,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        self.0.send(local, remote, data).await
    }

    async fn receive_into(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, SocketAddr, SocketAddr, [u8; 6]), Self::Error> {
        let (len, local, remote) = self.0.receive_into(buffer).await?;

        Ok((len, local, remote, [0x00; 6]))
    }
}

/// A utility method that binds a UDP socket in a way suitable for operating as a DHCP client or server.
pub async fn bind<T>(stack: &T, socket: SocketAddrV4) -> Result<T::MultiplyBound, Error<T::Error>>
where
    T: UdpStack,
{
    let socket = stack
        .bind_multiple(SocketAddr::V4(socket))
        .await
        .map_err(Error::Io)?;

    Ok(socket)
}
