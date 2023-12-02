use crate::dhcp;

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

pub mod client {
    use core::fmt::Debug;

    use embassy_futures::select::{self, select, Either};
    use embassy_time::{Duration, Instant, Timer};

    use embedded_io_async::{Read, Write};

    use embedded_nal_async::Ipv4Addr;

    use log::{info, trace, warn};

    use rand_core::RngCore;

    use self::dhcp::{MessageType, Options, Packet};

    pub use super::*;
    pub use crate::dhcp::Settings;

    use crate::{
        asynch::tcp::{RawSocket, RawStack, IO},
        dhcp::raw_ip::{Ipv4PacketHeader, UdpPacketHeader},
    };

    #[derive(Clone, Debug)]
    pub struct Configuration {
        pub mac: [u8; 6],
        pub client_port: Option<u16>,
        pub server_port: u16,
        pub timeout: Duration,
    }

    impl Configuration {
        pub const fn new(mac: [u8; 6]) -> Self {
            Self {
                mac,
                client_port: Some(68),
                server_port: 67,
                timeout: Duration::from_secs(10),
            }
        }
    }

    pub struct Client<T> {
        client: dhcp::client::Client<T>,
        client_port: Option<u16>,
        server_port: u16,
        timeout: Duration,
        settings: Option<(Settings, Instant)>,
    }

    impl<T> Client<T>
    where
        T: RngCore,
    {
        pub fn new(rng: T, conf: &Configuration) -> Self {
            info!("Creating DHCP client with configuration {conf:?}");

            Self {
                client: dhcp::client::Client {
                    rng,
                    mac: conf.mac,
                    raw_packets: None,
                },
                client_port: conf.client_port,
                server_port: conf.server_port,
                timeout: conf.timeout,
                settings: None,
            }
        }

        pub async fn run<R: RawStack>(
            &mut self,
            stack: R,
            buf: &mut [u8],
        ) -> Result<Option<Settings>, Error<R::Error>> {
            loop {
                if let Some((settings, acquired)) = self.settings.as_ref() {
                    // Keep the lease
                    let now = Instant::now();

                    if now - *acquired
                        < Duration::from_secs(settings.lease_time_secs.unwrap_or(7200) / 3 as _)
                    {
                        info!("Renewing DHCP lease...");

                        if let Some(settings) = self
                            .request(stack, buf, settings.server_ip.unwrap(), settings.ip)
                            .await?
                        {
                            self.settings = Some((settings, Instant::now()));
                        } else {
                            // Lease was not renewed; let the user know
                            self.settings = None;

                            return Ok(None);
                        }
                    } else {
                        Timer::after(Duration::from_secs(60)).await;
                    }
                } else {
                    // Look for offers
                    let offer = self.discover(stack, buf).await?;

                    if let Some(settings) = self
                        .request(stack, buf, offer.server_ip.unwrap(), offer.ip)
                        .await?
                    {
                        // IP acquired; let the user know
                        self.settings = Some((settings.clone(), Instant::now()));

                        return Ok(Some(settings));
                    }
                }
            }
        }

        pub async fn release<R: RawStack>(
            &mut self,
            stack: R,
            buf: &mut [u8],
        ) -> Result<(), Error<R::Error>> {
            if let Some((settings, _)) = self.settings.as_ref() {
                let mut socket = stack.connect(Some(*if_id)).await?;

                let packet =
                    self.client
                        .release(&mut buf, 0, settings.server_ip.unwrap(), settings.ip)?;

                socket.send(packet).await?;
            }

            self.settings = None;

            Ok(())
        }

        async fn discover<R: RawStack>(
            &mut self,
            stack: R,
            buf: &mut [u8],
        ) -> Result<Settings, Error<R::Error>> {
            info!("Discovering DHCP servers...");

            let start = Instant::now();

            loop {
                let mut socket = stack.connect(Some(*if_id)).await?;

                let (packet, xid) = self.client.discover(
                    &mut buf,
                    (Instant::now() - start).as_secs() as _,
                    None,
                )?;

                socket.send(packet).await?;

                let offer_start = Instant::now();

                while Instant::now() - offer_start < self.timeout {
                    let timer = Timer::after(Duration::from_secs(10));

                    if let Either::First(result) = select(socket.receive_into(buf), timer) {
                        let len = result?;
                        let packet = &buf[..len];

                        if let Some(reply) =
                            self.client.recv(packet, xid, Some(&[MessageType::Offer]))?
                        {
                            let settings = reply.settings().unwrap().1;

                            info!(
                                "IP {} offered by DHCP server {}",
                                settings.ip,
                                settings.server_ip.unwrap()
                            );
                            return Ok(settings);
                        }
                    }
                }

                drop(socket);

                info!("No DHCP offers received, sleeping for a while...");

                Timer::after(Duration::from_secs(10)).await;
            }
        }

        async fn request<R: RawStack>(
            &mut self,
            stack: R,
            buf: &mut [u8],
            server_ip: Ipv4Addr,
            ip: Ipv4Addr,
        ) -> Result<Option<Settings>, Error<W::Error>> {
            for _ in 0..3 {
                info!("Requesting IP {ip} from DHCP server {server_ip}");

                let mut socket = stack.connect(Some(*if_id)).await?;

                let start = Instant::now();

                let (packet, xid) = self.client.request(
                    &mut buf,
                    (Instant::now() - start).as_secs() as _,
                    server_ip,
                    ip,
                )?;

                socket.send(packet).await?;

                let request_start = Instant::now();

                while Instant::now() - request_start < self.timeout {
                    let timer = Timer::after(Duration::from_secs(10));

                    if let Either::First(result) = select(socket.receive_into(buf), timer) {
                        let len = result?;
                        let packet = &buf[..len];

                        if let Some(reply) = self.client.recv(
                            packet,
                            xid,
                            Some(&[MessageType::Ack, MessageType::Nak]),
                        )? {
                            let (mt, settings) = reply.settings().unwrap();

                            let settings = if matches!(mt, MessageType::Ack) {
                                info!("IP {} leased successfully", settings.ip);
                                Some(settings)
                            } else {
                                info!("IP {} not acknowledged", settings.ip);
                                None
                            };

                            return Ok(settings);
                        }
                    }
                }

                drop(socket);
            }

            warn!("IP request was not replied");

            Ok(None)
        }
    }
}

pub mod server {
    use core::fmt::Debug;

    use embassy_time::{Duration, Instant};

    use embedded_io_async::{Read, Write};

    use embedded_nal_async::{IpAddr, Ipv4Addr, SocketAddr, UdpStack, UnconnectedUdp};

    use crate::dhcp::{DhcpOption, MessageType, Options, Packet};

    pub use super::*;

    #[derive(Clone, Debug)]
    pub struct Configuration {
        pub ip: Ipv4Addr,
        pub gateway: Option<Ipv4Addr>,
        pub subnet: Option<Ipv4Addr>,
        pub dns1: Option<Ipv4Addr>,
        pub dns2: Option<Ipv4Addr>,
        pub range_start: Ipv4Addr,
        pub range_end: Ipv4Addr,
        pub lease_duration_secs: u32,
    }

    struct Lease {
        mac: [u8; 16],
        expires: Instant,
    }

    pub struct Server<const N: usize> {
        ip: Ipv4Addr,
        gateways: heapless::Vec<Ipv4Addr, 1>,
        subnet: Option<Ipv4Addr>,
        dns: heapless::Vec<Ipv4Addr, 2>,
        range_start: Ipv4Addr,
        range_end: Ipv4Addr,
        lease_duration: Duration,
        leases: heapless::LinearMap<Ipv4Addr, Lease, N>,
    }

    impl<const N: usize> Server<N> {
        pub fn new(conf: &Configuration) -> Self {
            Self {
                ip: conf.ip,
                gateways: conf.gateway.iter().cloned().collect(),
                subnet: conf.subnet,
                dns: conf.dns1.iter().chain(conf.dns2.iter()).cloned().collect(),
                range_start: conf.range_start,
                range_end: conf.range_end,
                lease_duration: Duration::from_secs(conf.lease_duration_secs as _),
                leases: heapless::LinearMap::new(),
            }
        }

        pub async fn run<U: UdpStack>(
            &mut self,
            udp: &mut U,
            buf: &mut [u8],
        ) -> Result<(), Error<U::Error>> {
            let mut socket = udp
                .bind_multiple(SocketAddr::new(IpAddr::V4(self.ip), 67))
                .await
                .map_err(Error::Io)?;

            loop {
                self.handle::<U>(&mut socket, buf).await?;
            }
        }

        pub async fn read<R: Read>(
            &mut self,
            read: R,
            buf: &mut [u8],
        ) -> Result<(), Error<R::Error>> {
            let (len, local_addr, remote_addr) =
                socket.receive_into(buf).await.map_err(Error::Io)?;
        }

        pub async fn handle<'o, W: Write>(
            &mut self,
            mut write: W,
            buf: &'o mut [u8],
            incoming_len: usize,
        ) -> Result<(), Error<W::Error>> {
            let request = Packet::decode(&buf[..incoming_len])?;

            if !request.reply {
                let mt = request.options.iter().find_map(|option| {
                    if let DhcpOption::MessageType(mt) = option {
                        Some(mt)
                    } else {
                        None
                    }
                });

                if let Some(mt) = mt {
                    let server_identifier = request.options.iter().find_map(|option| {
                        if let DhcpOption::ServerIdentifier(ip) = option {
                            Some(ip)
                        } else {
                            None
                        }
                    });

                    if server_identifier == Some(self.ip)
                        || server_identifier.is_none() && matches!(mt, MessageType::Discover)
                    {
                        let mut opt_buf = Options::buf();

                        let reply = match mt {
                            MessageType::Discover => {
                                let requested_ip = request.options.iter().find_map(|option| {
                                    if let DhcpOption::RequestedIpAddress(ip) = option {
                                        Some(ip)
                                    } else {
                                        None
                                    }
                                });

                                let ip = requested_ip
                                    .and_then(|ip| {
                                        self.is_available(&request.chaddr, ip).then_some(ip)
                                    })
                                    .or_else(|| self.current_lease(&request.chaddr))
                                    .or_else(|| self.available());

                                ip.map(|ip| {
                                    self.reply_to(
                                        &request,
                                        MessageType::Offer,
                                        Some(ip),
                                        &mut opt_buf,
                                    )
                                })
                            }
                            MessageType::Request => {
                                let ip = request
                                    .options
                                    .iter()
                                    .find_map(|option| {
                                        if let DhcpOption::RequestedIpAddress(ip) = option {
                                            Some(ip)
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(request.ciaddr);

                                Some(
                                    if self.is_available(&request.chaddr, ip)
                                        && self.add_lease(
                                            ip,
                                            request.chaddr,
                                            Instant::now() + self.lease_duration,
                                        )
                                    {
                                        self.reply_to(
                                            &request,
                                            MessageType::Ack,
                                            Some(ip),
                                            &mut opt_buf,
                                        )
                                    } else {
                                        self.reply_to(
                                            &request,
                                            MessageType::Nak,
                                            None,
                                            &mut opt_buf,
                                        )
                                    },
                                )
                            }
                            MessageType::Decline | MessageType::Release => {
                                self.remove_lease(&request.chaddr);

                                None
                            }
                            _ => None,
                        };

                        if let Some(reply) = reply {
                            let data = reply.encode(buf)?;

                            write.write_all(data).await.map_err(Error::Io)?;
                        }
                    }
                }
            }

            Ok(())
        }

        fn reply_to<'a>(
            &'a self,
            request: &Packet<'_>,
            mt: MessageType,
            ip: Option<Ipv4Addr>,
            buf: &'a mut [DhcpOption<'a>],
        ) -> Packet<'a> {
            request.new_reply(
                ip,
                request.options.reply(
                    mt,
                    self.ip,
                    self.lease_duration.as_secs() as _,
                    &self.gateways,
                    self.subnet,
                    &self.dns,
                    buf,
                ),
            )
        }

        fn is_available(&self, mac: &[u8; 16], addr: Ipv4Addr) -> bool {
            let pos: u32 = addr.into();

            let start: u32 = self.range_start.into();
            let end: u32 = self.range_end.into();

            pos >= start
                && pos <= end
                && match self.leases.get(&addr) {
                    Some(lease) => lease.mac == *mac || Instant::now() > lease.expires,
                    None => true,
                }
        }

        fn available(&mut self) -> Option<Ipv4Addr> {
            let start: u32 = self.range_start.into();
            let end: u32 = self.range_end.into();

            for pos in start..end + 1 {
                let addr = pos.into();

                if !self.leases.contains_key(&addr) {
                    return Some(addr);
                }
            }

            if let Some(addr) = self
                .leases
                .iter()
                .find_map(|(addr, lease)| (Instant::now() > lease.expires).then_some(*addr))
            {
                self.leases.remove(&addr);

                Some(addr)
            } else {
                None
            }
        }

        fn current_lease(&self, mac: &[u8; 16]) -> Option<Ipv4Addr> {
            self.leases
                .iter()
                .find_map(|(addr, lease)| (lease.mac == *mac).then_some(*addr))
        }

        fn add_lease(&mut self, addr: Ipv4Addr, mac: [u8; 16], expires: Instant) -> bool {
            self.remove_lease(&mac);

            self.leases.insert(addr, Lease { mac, expires }).is_ok()
        }

        fn remove_lease(&mut self, mac: &[u8; 16]) -> bool {
            if let Some(addr) = self.current_lease(mac) {
                self.leases.remove(&addr);

                true
            } else {
                false
            }
        }
    }
}
