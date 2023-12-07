use core::{fmt::Debug, mem::MaybeUninit};

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

pub struct ConnectedUdp2RawSocket<T, const N: usize>(T, SocketAddrV4, SocketAddrV4);

impl<T, const N: usize> ConnectedUdp for ConnectedUdp2RawSocket<T, N>
where
    T: RawSocket,
{
    type Error = Error<T::Error>;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        send::<_, N>(
            &mut self.0,
            SocketAddr::V4(self.1),
            SocketAddr::V4(self.2),
            data,
        )
        .await
    }

    async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let (len, _, _) =
            receive_into::<_, N>(&mut self.0, Some(self.1), Some(self.2), buffer).await?;

        Ok(len)
    }
}

pub struct UnconnectedUdp2RawSocket<T, const N: usize>(T, Option<SocketAddrV4>);

impl<T, const N: usize> UnconnectedUdp for UnconnectedUdp2RawSocket<T, N>
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
        send::<_, N>(&mut self.0, local, remote, data).await
    }

    async fn receive_into(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, SocketAddr, SocketAddr), Self::Error> {
        receive_into::<_, N>(&mut self.0, None, self.1, buffer).await
    }
}

pub struct Udp2RawStack<T, const N: usize = 1500>(T, T::Interface)
where
    T: RawStack;

impl<T, const N: usize> Udp2RawStack<T, N>
where
    T: RawStack,
{
    pub const fn new(stack: T, interface: T::Interface) -> Self {
        Self(stack, interface)
    }
}

impl<T, const N: usize> UdpStack for Udp2RawStack<T, N>
where
    T: RawStack,
{
    type Error = Error<T::Error>;

    type Connected = ConnectedUdp2RawSocket<T::Socket, N>;

    type UniquelyBound = UnconnectedUdp2RawSocket<T::Socket, N>;

    type MultiplyBound = UnconnectedUdp2RawSocket<T::Socket, N>;

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
        let SocketAddr::V4(localv4) = local else {
            Err(Error::UnsupportedProtocol)?
        };

        let socket = self.0.bind(&self.1).await.map_err(Self::Error::Io)?;

        Ok(UnconnectedUdp2RawSocket(socket, Some(localv4)))
    }
}

async fn send<T: RawSocket, const N: usize>(
    mut socket: T,
    local: SocketAddr,
    remote: SocketAddr,
    data: &[u8],
) -> Result<(), Error<T::Error>> {
    let (SocketAddr::V4(local), SocketAddr::V4(remote)) = (local, remote) else {
        Err(Error::UnsupportedProtocol)?
    };

    let mut buf = MaybeUninit::<[u8; N]>::uninit();
    let buf = unsafe { buf.assume_init_mut() };

    let data = raw::ip_udp_encode(buf, local, remote, |buf| {
        if data.len() <= buf.len() {
            buf[..data.len()].copy_from_slice(data);

            Ok(data.len())
        } else {
            Err(raw::Error::BufferOverflow)
        }
    })?;

    socket.send(data).await.map_err(Error::Io)
}

async fn receive_into<T: RawSocket, const N: usize>(
    mut socket: T,
    filter_src: Option<SocketAddrV4>,
    filter_dst: Option<SocketAddrV4>,
    buffer: &mut [u8],
) -> Result<(usize, SocketAddr, SocketAddr), Error<T::Error>> {
    let mut buf = MaybeUninit::<[u8; N]>::uninit();
    let buf = unsafe { buf.assume_init_mut() };

    let (local, remote, len) = loop {
        let len = socket.receive_into(buf).await.map_err(Error::Io)?;

        match raw::ip_udp_decode(&buf[..len], filter_src, filter_dst) {
            Ok(Some((remote, local, data))) => {
                if data.len() > buffer.len() {
                    Err(Error::RawError(raw::Error::BufferOverflow))?;
                }

                buffer[..data.len()].copy_from_slice(data);

                break (local, remote, data.len());
            }
            Ok(None) => continue,
            Err(raw::Error::InvalidFormat) | Err(raw::Error::InvalidChecksum) => continue,
            Err(other) => Err(other)?,
        }
    };

    Ok((len, SocketAddr::V4(local), SocketAddr::V4(remote)))
}
