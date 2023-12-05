use core::fmt::Debug;

use embedded_io_async::ErrorKind;

use embedded_nal_async::{ConnectedUdp, SocketAddr, SocketAddrV4, UdpStack, UnconnectedUdp};

use embedded_nal_async_xtra::{RawSocket, RawStack};

use crate as raw;

#[derive(Debug)]
pub enum Error<E> {
    Io(E),
    UnsupportedProtocol,
    RawError(raw::Error),
}

impl<E> From<raw::Error> for Error<E> {
    fn from(value: raw::Error) -> Self {
        Self::RawError(value)
    }
}

impl<E> embedded_io_async::Error for Error<E>
where
    E: embedded_io_async::Error,
{
    fn kind(&self) -> ErrorKind {
        match self {
            Self::Io(err) => err.kind(),
            Self::UnsupportedProtocol => ErrorKind::InvalidInput,
            Self::RawError(_) => ErrorKind::InvalidData,
        }
    }
}

pub struct ConnectedUdp2RawSocket<T>(T, SocketAddrV4, SocketAddrV4);

impl<T> ConnectedUdp for ConnectedUdp2RawSocket<T>
where
    T: RawSocket,
{
    type Error = Error<T::Error>;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        send(
            &mut self.0,
            SocketAddr::V4(self.1),
            SocketAddr::V4(self.2),
            data,
        )
        .await
    }

    async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let (len, _, _) = receive_into(&mut self.0, Some(self.1), Some(self.2), buffer).await?;

        Ok(len)
    }
}

pub struct UnconnectedUdp2RawSocket<T>(T, Option<SocketAddrV4>);

impl<T> UnconnectedUdp for UnconnectedUdp2RawSocket<T>
where
    T: RawSocket,
{
    type Error = Error<T::Error>;

    async fn send(
        &mut self,
        local: SocketAddr,
        remote: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        send(&mut self.0, local, remote, data).await
    }

    async fn receive_into(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, SocketAddr, SocketAddr), Self::Error> {
        receive_into(&mut self.0, None, self.1, buffer).await
    }
}

pub struct Udp2RawStack<T>(T, T::Interface)
where
    T: RawStack;

impl<T> UdpStack for Udp2RawStack<T>
where
    T: RawStack,
{
    type Error = Error<T::Error>;

    type Connected = ConnectedUdp2RawSocket<T::Socket>;

    type UniquelyBound = UnconnectedUdp2RawSocket<T::Socket>;

    type MultiplyBound = UnconnectedUdp2RawSocket<T::Socket>;

    async fn connect_from(
        &self,
        local: SocketAddr,
        remote: SocketAddr,
    ) -> Result<(SocketAddr, Self::Connected), Self::Error> {
        let (SocketAddr::V4(localv4), SocketAddr::V4(remotev4)) = (local, remote) else {
            Err(Error::UnsupportedProtocol)?
        };

        let socket = self.0.bind(&self.1).await.map_err(Self::Error::Io)?;

        Ok((local, ConnectedUdp2RawSocket(socket, localv4, remotev4)))
    }

    async fn bind_single(
        &self,
        local: SocketAddr,
    ) -> Result<(SocketAddr, Self::UniquelyBound), Self::Error> {
        let SocketAddr::V4(localv4) = local else {
            Err(Error::UnsupportedProtocol)?
        };

        let socket = self.0.bind(&self.1).await.map_err(Self::Error::Io)?;

        Ok((local, UnconnectedUdp2RawSocket(socket, Some(localv4))))
    }

    async fn bind_multiple(&self, local: SocketAddr) -> Result<Self::MultiplyBound, Self::Error> {
        let SocketAddr::V4(local) = local else {
            Err(Error::UnsupportedProtocol)?
        };

        let socket = self.0.bind(&self.1).await.map_err(Self::Error::Io)?;

        Ok(UnconnectedUdp2RawSocket(socket, Some(local)))
    }
}

async fn send<T: RawSocket>(
    mut socket: T,
    local: SocketAddr,
    remote: SocketAddr,
    data: &[u8],
) -> Result<(), Error<T::Error>> {
    let (SocketAddr::V4(local), SocketAddr::V4(remote)) = (local, remote) else {
        Err(Error::UnsupportedProtocol)?
    };

    let mut buf = [0; 1500];

    let data = raw::ip_udp_encode(&mut buf, local, remote, |buf| {
        if data.len() <= buf.len() {
            buf[..data.len()].copy_from_slice(data);

            Ok(data.len())
        } else {
            Err(raw::Error::BufferOverflow)
        }
    })?;

    socket.send(data).await.map_err(Error::Io)
}

async fn receive_into<T: RawSocket>(
    mut socket: T,
    filter_src: Option<SocketAddrV4>,
    filter_dst: Option<SocketAddrV4>,
    buffer: &mut [u8],
) -> Result<(usize, SocketAddr, SocketAddr), Error<T::Error>> {
    let mut buf = [0; 1500];

    let (local, remote, len) = loop {
        let len = socket.receive_into(&mut buf).await.map_err(Error::Io)?;

        match raw::ip_udp_decode(&buf[..len], filter_src, filter_dst) {
            Ok(Some((local, remote, data))) => break (local, remote, data.len()),
            Ok(None) => continue,
            Err(raw::Error::InvalidFormat) | Err(raw::Error::InvalidChecksum) => continue,
            Err(other) => Err(other)?,
        }
    };

    if len <= buffer.len() {
        buffer[..len].copy_from_slice(&buf[..len]);

        Ok((len, SocketAddr::V4(local), SocketAddr::V4(remote)))
    } else {
        Err(raw::Error::BufferOverflow.into())
    }
}
