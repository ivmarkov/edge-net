#![allow(async_fn_in_trait)]
#![warn(clippy::large_futures)]

use core::net::{IpAddr, SocketAddr};
use core::pin::pin;

use std::io;
use std::net::{self, TcpStream, ToSocketAddrs, UdpSocket as StdUdpSocket};

use async_io::Async;

use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use embedded_io_async::{ErrorType, Read, Write};

use edge_nal::{
    AddrType, Dns, Multicast, TcpAccept, TcpBind, TcpConnect, TcpSplit, UdpBind, UdpConnect,
    UdpReceive, UdpSend, UdpSplit,
};

#[cfg(all(unix, not(target_os = "espidf")))]
pub use raw::*;

#[derive(Default)]
pub struct Stack(());

impl Stack {
    pub const fn new() -> Self {
        Self(())
    }
}

impl TcpConnect for Stack {
    type Error = io::Error;

    type Socket<'a> = TcpSocket where Self: 'a;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        let socket = Async::<TcpStream>::connect(remote).await?;

        Ok(TcpSocket(socket))
    }
}

impl TcpBind for Stack {
    type Error = io::Error;

    type Accept<'a> = TcpAcceptor where Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<Self::Accept<'_>, Self::Error> {
        let acceptor = Async::<net::TcpListener>::bind(local).map(TcpAcceptor)?;

        Ok(acceptor)
    }
}

pub struct TcpAcceptor(Async<net::TcpListener>);

impl TcpAccept for TcpAcceptor {
    type Error = io::Error;

    type Socket<'a> = TcpSocket where Self: 'a;

    #[cfg(not(target_os = "espidf"))]
    async fn accept(&self) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        let socket = self.0.accept().await.map(|(socket, _)| socket)?;

        Ok((socket.as_ref().peer_addr()?, TcpSocket(socket)))
    }

    #[cfg(target_os = "espidf")]
    async fn accept(&self) -> Result<(SocketAddr, Self::Socket<'_>), Self::Error> {
        // ESP IDF (lwIP actually) does not really support `select`-ing on
        // socket accept: https://groups.google.com/g/osdeve_mirror_tcpip_lwip/c/Vsz7SVa6a2M
        //
        // If we do this, `select` would block and not return with our accepting socket `fd`
        // marked as ready even if our accepting socket has a pending connection.
        //
        // (Note also that since the time when the above link was posted on the internet,
        // the lwIP `accept` API has improved a bit in that it would now return `EWOULDBLOCK`
        // instead of blocking indefinitely
        // - and we take advantage of that in the "async" implementation below.)
        //
        // The workaround below is not ideal in that
        // it uses a timer to poll the socket, but it avoids spinning a hidden,
        // separate thread just to accept connections - which would be the alternative.
        loop {
            match self.0.as_ref().accept() {
                Ok((socket, _)) => {
                    break Ok((socket.as_ref().peer_addr()?, TcpSocket(Async::new(socket)?)))
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    async_io::Timer::after(core::time::Duration::from_millis(5)).await;
                }
                Err(err) => break Err(err),
            }
        }
    }
}

pub struct TcpSocket(Async<TcpStream>);

impl ErrorType for TcpSocket {
    type Error = io::Error;
}

impl Read for TcpSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await
    }
}

impl Write for TcpSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await
    }
}

impl ErrorType for &TcpSocket {
    type Error = io::Error;
}

impl Read for &TcpSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        (&self.0).read(buf).await
    }
}

impl Write for &TcpSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        (&self.0).write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        (&self.0).flush().await
    }
}

impl TcpSplit for TcpSocket {
    type Read<'a> = &'a TcpSocket where Self: 'a;

    type Write<'a> = &'a TcpSocket where Self: 'a;

    fn split(&mut self) -> (Self::Read<'_>, Self::Write<'_>) {
        let socket = &*self;

        (socket, socket)
    }
}

impl UdpConnect for Stack {
    type Error = io::Error;

    type Socket<'a> = UdpSocket where Self: 'a;

    async fn connect(
        &self,
        local: SocketAddr,
        remote: SocketAddr,
    ) -> Result<Self::Socket<'_>, Self::Error> {
        let socket = Async::<StdUdpSocket>::bind(local)?;

        socket.as_ref().connect(remote)?;

        Ok(UdpSocket(socket))
    }
}

impl UdpBind for Stack {
    type Error = io::Error;

    type Socket<'a> = UdpSocket where Self: 'a;

    async fn bind(&self, local: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        let socket = Async::<StdUdpSocket>::bind(local)?;

        socket.as_ref().set_broadcast(true)?;

        Ok(UdpSocket(socket))
    }
}

pub struct UdpSocket(Async<StdUdpSocket>);

impl ErrorType for &UdpSocket {
    type Error = io::Error;
}

impl UdpReceive for &UdpSocket {
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, SocketAddr), Self::Error> {
        let remote = self.0.as_ref().peer_addr();

        let (len, remote) = if let Ok(remote) = remote {
            // Connected socket
            let fut = pin!(self.0.recv(buffer));
            let len = fut.await?;

            (len, remote)
        } else {
            // Unconnected socket
            let fut = pin!(self.0.recv_from(buffer));
            let (len, remote) = fut.await?;

            (len, remote)
        };

        Ok((len, remote))
    }
}

impl UdpSend for &UdpSocket {
    async fn send(&mut self, remote: SocketAddr, data: &[u8]) -> Result<(), Self::Error> {
        let is_remote = self.0.as_ref().peer_addr().is_ok();

        if is_remote {
            // Connected socket
            let mut offset = 0;

            loop {
                let fut = pin!(self.0.send(&data[offset..]));
                offset += fut.await?;

                if offset == data.len() {
                    break;
                }
            }
        } else {
            // Unconnected socket
            let mut offset = 0;

            loop {
                let fut = pin!(self.0.send_to(&data[offset..], remote));
                offset += fut.await?;

                if offset == data.len() {
                    break;
                }
            }
        }

        Ok(())
    }
}

impl ErrorType for UdpSocket {
    type Error = io::Error;
}

impl UdpReceive for UdpSocket {
    async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, SocketAddr), Self::Error> {
        let mut rself = &*self;

        let fut = pin!(rself.receive(buffer));
        fut.await
    }
}

impl UdpSend for UdpSocket {
    async fn send(&mut self, remote: SocketAddr, data: &[u8]) -> Result<(), Self::Error> {
        let mut rself = &*self;

        let fut = pin!(rself.send(remote, data));
        fut.await
    }
}

impl Multicast for UdpSocket {
    async fn join(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error> {
        match multicast_addr {
            IpAddr::V4(addr) => self
                .0
                .as_ref()
                .join_multicast_v4(&addr.octets().into(), &std::net::Ipv4Addr::UNSPECIFIED)?,
            IpAddr::V6(addr) => self
                .0
                .as_ref()
                .join_multicast_v6(&addr.octets().into(), 0)?,
        }

        Ok(())
    }

    async fn leave(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error> {
        match multicast_addr {
            IpAddr::V4(addr) => self
                .0
                .as_ref()
                .leave_multicast_v4(&addr.octets().into(), &std::net::Ipv4Addr::UNSPECIFIED)?,
            IpAddr::V6(addr) => self
                .0
                .as_ref()
                .leave_multicast_v6(&addr.octets().into(), 0)?,
        }

        Ok(())
    }
}

impl UdpSplit for UdpSocket {
    type Receive<'a> = &'a Self where Self: 'a;

    type Send<'a> = &'a Self where Self: 'a;

    fn split(&mut self) -> (Self::Receive<'_>, Self::Send<'_>) {
        let socket = &*self;

        (socket, socket)
    }
}

impl Dns for Stack {
    type Error = io::Error;

    async fn get_host_by_name(
        &self,
        host: &str,
        addr_type: AddrType,
    ) -> Result<IpAddr, Self::Error> {
        let host = host.to_string();

        dns_lookup_host(&host, addr_type)
    }

    async fn get_host_by_address(
        &self,
        _addr: IpAddr,
        _result: &mut [u8],
    ) -> Result<usize, Self::Error> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

fn dns_lookup_host(host: &str, addr_type: AddrType) -> Result<IpAddr, io::Error> {
    (host, 0_u16)
        .to_socket_addrs()?
        .find(|addr| match addr_type {
            AddrType::IPv4 => matches!(addr, std::net::SocketAddr::V4(_)),
            AddrType::IPv6 => matches!(addr, std::net::SocketAddr::V6(_)),
            AddrType::Either => true,
        })
        .map(|addr| match addr {
            std::net::SocketAddr::V4(v4) => v4.ip().octets().into(),
            std::net::SocketAddr::V6(v6) => v6.ip().octets().into(),
        })
        .ok_or_else(|| io::ErrorKind::AddrNotAvailable.into())
}

#[cfg(all(unix, not(target_os = "espidf")))]
mod raw {
    use core::pin::pin;

    use std::io::{self, ErrorKind};
    use std::os::fd::{AsFd, AsRawFd};

    use async_io::Async;

    use edge_nal::{MacAddr, RawBind, RawReceive, RawSend, RawSplit};
    use embedded_io_async::ErrorType;

    #[derive(Default)]
    pub struct Interface(u32);

    impl Interface {
        pub const fn new(interface: u32) -> Self {
            Self(interface)
        }
    }

    impl RawBind for Interface {
        type Error = io::Error;

        type Socket<'a> = RawSocket where Self: 'a;

        async fn bind(&self) -> Result<Self::Socket<'_>, Self::Error> {
            let socket = cvt(unsafe {
                libc::socket(
                    libc::PF_PACKET,
                    libc::SOCK_DGRAM,
                    (libc::ETH_P_IP as u16).to_be() as _,
                )
            })?;

            let sockaddr = libc::sockaddr_ll {
                sll_family: libc::AF_PACKET as _,
                sll_protocol: (libc::ETH_P_IP as u16).to_be() as _,
                sll_ifindex: self.0 as _,
                sll_hatype: 0,
                sll_pkttype: 0,
                sll_halen: 0,
                sll_addr: Default::default(),
            };

            cvt(unsafe {
                libc::bind(
                    socket,
                    &sockaddr as *const _ as *const _,
                    core::mem::size_of::<libc::sockaddr_ll>() as _,
                )
            })?;

            // TODO
            // cvt(unsafe {
            //     libc::setsockopt(socket, libc::SOL_PACKET, libc::PACKET_AUXDATA, &1_u32 as *const _ as *const _, 4)
            // })?;

            let socket = {
                use std::os::fd::FromRawFd;

                unsafe { std::net::UdpSocket::from_raw_fd(socket) }
            };

            socket.set_broadcast(true)?;

            Ok(RawSocket(Async::new(socket)?, self.0 as _))
        }
    }

    pub struct RawSocket(Async<std::net::UdpSocket>, u32);

    impl ErrorType for &RawSocket {
        type Error = io::Error;
    }

    impl RawReceive for &RawSocket {
        async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, MacAddr), Self::Error> {
            let fut = pin!(self.0.read_with(|io| {
                let mut storage: libc::sockaddr_storage = unsafe { core::mem::zeroed() };
                let mut addrlen = core::mem::size_of_val(&storage) as libc::socklen_t;

                let ret = cvti(unsafe {
                    libc::recvfrom(
                        io.as_fd().as_raw_fd(),
                        buffer.as_mut_ptr() as *mut _,
                        buffer.len(),
                        0,
                        &mut storage as *mut _ as *mut _,
                        &mut addrlen,
                    )
                })?;

                let sockaddr = as_sockaddr_ll(&storage, addrlen as usize)?;

                let mut mac = [0; 6];
                mac.copy_from_slice(&sockaddr.sll_addr[..6]);

                Ok((ret as usize, mac))
            }));

            fut.await
        }
    }

    impl RawSend for &RawSocket {
        async fn send(&mut self, mac: MacAddr, data: &[u8]) -> Result<(), Self::Error> {
            let mut sockaddr = libc::sockaddr_ll {
                sll_family: libc::AF_PACKET as _,
                sll_protocol: (libc::ETH_P_IP as u16).to_be() as _,
                sll_ifindex: self.1 as _,
                sll_hatype: 0,
                sll_pkttype: 0,
                sll_halen: 0,
                sll_addr: Default::default(),
            };

            sockaddr.sll_halen = mac.len() as _;
            sockaddr.sll_addr[..mac.len()].copy_from_slice(&mac);

            let fut = pin!(self.0.write_with(|io| {
                let len = core::cmp::min(data.len(), u16::MAX as usize);

                let ret = cvti(unsafe {
                    libc::sendto(
                        io.as_fd().as_raw_fd(),
                        data.as_ptr() as *const _,
                        len,
                        libc::MSG_NOSIGNAL,
                        &sockaddr as *const _ as *const _,
                        core::mem::size_of::<libc::sockaddr_ll>() as _,
                    )
                })?;
                Ok(ret as usize)
            }));

            let len = fut.await?;

            assert_eq!(len, data.len());

            Ok(())
        }
    }

    impl ErrorType for RawSocket {
        type Error = io::Error;
    }

    impl RawReceive for RawSocket {
        async fn receive(&mut self, buffer: &mut [u8]) -> Result<(usize, MacAddr), Self::Error> {
            let mut rself = &*self;

            let fut = pin!(rself.receive(buffer));

            fut.await
        }
    }

    impl RawSend for RawSocket {
        async fn send(&mut self, mac: MacAddr, data: &[u8]) -> Result<(), Self::Error> {
            let mut rself = &*self;

            let fut = pin!(rself.send(mac, data));

            fut.await
        }
    }

    impl RawSplit for RawSocket {
        type Receive<'a> = &'a Self where Self: 'a;

        type Send<'a> = &'a Self where Self: 'a;

        fn split(&mut self) -> (Self::Receive<'_>, Self::Send<'_>) {
            let socket = &*self;

            (socket, socket)
        }
    }

    fn as_sockaddr_ll(
        storage: &libc::sockaddr_storage,
        len: usize,
    ) -> io::Result<&libc::sockaddr_ll> {
        match storage.ss_family as core::ffi::c_int {
            libc::AF_PACKET => {
                assert!(len >= core::mem::size_of::<libc::sockaddr_ll>());
                Ok(unsafe { (storage as *const _ as *const libc::sockaddr_ll).as_ref() }.unwrap())
            }
            _ => Err(io::Error::new(ErrorKind::InvalidInput, "invalid argument")),
        }
    }

    fn cvt<T>(res: T) -> io::Result<T>
    where
        T: Into<i64> + Copy,
    {
        let ires: i64 = res.into();

        if ires == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }

    fn cvti<T>(res: T) -> io::Result<T>
    where
        T: Into<isize> + Copy,
    {
        let ires: isize = res.into();

        if ires == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }
}
