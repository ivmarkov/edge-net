use crate::dhcp;

#[derive(Debug)]
pub enum Error<E> {
    Io(E),
    Format(dhcp::Error),
    Timeout,
    Nak,
}

impl<E> From<dhcp::Error> for Error<E> {
    fn from(value: dhcp::Error) -> Self {
        Self::Format(value)
    }
}

pub mod client {
    use core::fmt::Debug;

    use embassy_futures::select::{select, Either};
    use embassy_time::{Duration, Instant, Timer};

    use embedded_nal_async::Ipv4Addr;

    use log::warn;
    use rand_core::RngCore;

    use self::dhcp::{MessageType, Options, Packet};

    pub use super::*;
    pub use crate::dhcp::Settings;

    use crate::dhcp::raw_ip::{Ipv4PacketHeader, UdpPacketHeader};

    // TODO: Ideally should go to `embedded-nal-async`
    pub trait RawSocket {
        type Error: Debug;

        async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error>;
        async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
    }

    // TODO: Ideally should go to `embedded-nal-async`
    pub trait RawStack {
        type Error: Debug;

        type Socket: RawSocket<Error = Self::Error>;

        async fn connect(&self, interface: Option<u32>) -> Result<Self::Socket, Self::Error>;
    }

    #[derive(Clone, Debug)]
    pub struct Configuration {
        pub mac: [u8; 6],
        pub interface: Option<u32>,
        pub retries: usize,
        pub timeout: Duration,
    }

    impl Configuration {
        pub const fn new(mac: [u8; 6]) -> Self {
            Self {
                mac,
                interface: None,
                retries: 10,
                timeout: Duration::from_secs(10),
            }
        }
    }

    pub struct Client<R> {
        rng: R,
        mac: [u8; 6],
        interface: Option<u32>,
        retries: usize,
        timeout: Duration,
    }

    impl<R> Client<R>
    where
        R: RngCore,
    {
        pub fn new(rng: R, conf: &Configuration) -> Self {
            Self {
                rng,
                mac: conf.mac,
                interface: conf.interface.and_then(|s| s.try_into().ok()),
                retries: conf.retries,
                timeout: conf.timeout,
            }
        }

        pub async fn discover<U: RawStack>(
            &mut self,
            udp: &mut U,
            buf: &mut [u8],
            ip: Option<Ipv4Addr>,
        ) -> Result<Settings, Error<U::Error>> {
            let mut opt_buf = Options::buf();

            let (_, settings) = self
                .send(
                    udp,
                    buf,
                    None,
                    None,
                    Options::discover(ip, &mut opt_buf),
                    &[MessageType::Offer],
                )
                .await?
                .unwrap();

            self.request(udp, buf, settings.server_ip.unwrap(), settings.ip)
                .await
        }

        pub async fn request<U: RawStack>(
            &mut self,
            udp: &mut U,
            buf: &mut [u8],
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<Settings, Error<U::Error>> {
            let mut opt_buf = Options::buf();

            let (mt, settings) = self
                .send(
                    udp,
                    buf,
                    Some(server_ip),
                    Some(our_ip),
                    Options::request(our_ip, &mut opt_buf),
                    &[MessageType::Ack, MessageType::Nak],
                )
                .await?
                .unwrap();

            if matches!(mt, MessageType::Ack) {
                Ok(settings)
            } else {
                Err(Error::Nak)
            }
        }

        pub async fn release<U: RawStack>(
            &mut self,
            udp: &mut U,
            buf: &mut [u8],
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<(), Error<U::Error>> {
            let mut opt_buf = Options::buf();

            self.send(
                udp,
                buf,
                Some(server_ip),
                Some(our_ip),
                Options::release(&mut opt_buf),
                &[],
            )
            .await?;

            Ok(())
        }

        pub async fn decline<U: RawStack>(
            &mut self,
            udp: &mut U,
            buf: &mut [u8],
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<(), Error<U::Error>> {
            let mut opt_buf = Options::buf();

            self.send(
                udp,
                buf,
                Some(server_ip),
                Some(our_ip),
                Options::decline(&mut opt_buf),
                &[],
            )
            .await?;

            Ok(())
        }

        async fn send<U: RawStack>(
            &mut self,
            udp: &mut U,
            buf: &mut [u8],
            server_ip: Option<Ipv4Addr>,
            our_ip: Option<Ipv4Addr>,
            options: Options<'_>,
            expected_message_types: &[MessageType],
        ) -> Result<Option<(MessageType, Settings)>, Error<U::Error>> {
            const PROTO_UDP: u8 = 17;
            const BROADCAST: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);

            if buf.len() < Ipv4PacketHeader::MIN_SIZE + UdpPacketHeader::SIZE {
                Err(Error::Format(dhcp::Error::BufferOverflow))?;
            }

            let start = Instant::now();

            let xid = self.rng.next_u32();

            for _ in 0..self.retries {
                let server_ip = server_ip.unwrap_or(BROADCAST);

                let mut socket: U::Socket = udp.connect(self.interface).await.map_err(Error::Io)?;

                let request = Packet::new_request(
                    self.mac,
                    xid,
                    (Instant::now() - start).as_secs() as _,
                    our_ip,
                    options.clone(),
                );

                let ip_packet_len = {
                    let (ip_hdr_buf, ip_data_buf) = buf.split_at_mut(Ipv4PacketHeader::MIN_SIZE);

                    let ip_data_len = {
                        let (udp_hdr_buf, udp_data_buf) =
                            ip_data_buf.split_at_mut(UdpPacketHeader::SIZE);

                        let data_len = request.encode(udp_data_buf)?.len();

                        warn!("DHCP packet encoded");

                        let mut udp_hdr = UdpPacketHeader::new(68, 67);
                        udp_hdr.len = UdpPacketHeader::SIZE as u16 + data_len as u16;

                        let l = udp_hdr.encode(udp_hdr_buf)?.len() + data_len;

                        warn!("UDP packet hdr encoded");

                        l
                    };

                    let mut ip_hdr = Ipv4PacketHeader::new(
                        our_ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
                        server_ip,
                        PROTO_UDP,
                    );

                    let checksum = UdpPacketHeader::checksum(&ip_data_buf[..ip_data_len], &ip_hdr);
                    UdpPacketHeader::inject_checksum(&mut ip_data_buf[..ip_data_len], checksum);

                    warn!("UDP packet checksum computed & injected");

                    ip_hdr.len = ip_hdr.hlen as u16 + ip_data_len as u16;

                    let l = ip_hdr.encode(ip_hdr_buf)?.len() + ip_data_len;

                    warn!("IP packet hdr encoded");

                    let checksum = Ipv4PacketHeader::checksum(ip_hdr_buf);
                    Ipv4PacketHeader::inject_checksum(ip_hdr_buf, checksum);

                    warn!("IP packet checksum computed & injected");

                    l
                };

                socket
                    .send(&buf[..ip_packet_len])
                    .await
                    .map_err(Error::Io)?;

                warn!("IP packet sent");

                if !expected_message_types.is_empty() {
                    warn!("Awaiting response packet");

                    loop {
                        let timer = Timer::after(self.timeout);

                        let len = match select(socket.receive_into(buf), timer).await {
                            Either::First(result) => result.map_err(Error::Io)?,
                            Either::Second(_) => break,
                        };

                        let data = &buf[..len];

                        warn!("Got packet");

                        let ip_hdr = Ipv4PacketHeader::decode(data)?;

                        warn!("IP header decoded");

                        if ip_hdr.version == 4 && ip_hdr.p == 17 {
                            // UDP on top of IPv4

                            warn!("IP packet is for us");

                            // TODO: Validate ipv4 header checksum

                            let (udp_hdr, udp_data) =
                                &data[ip_hdr.hlen as usize..].split_at(UdpPacketHeader::SIZE);

                            let udp_hdr = UdpPacketHeader::decode(udp_hdr)?;

                            warn!(
                                "UDP header decoded, src={}, dst={}",
                                udp_hdr.src, udp_hdr.dst
                            );

                            if udp_hdr.src == 67 && udp_hdr.dst == 68 {
                                // Reply from a DHCP server, on our port

                                warn!("UDP packet is for us");

                                // TODO: Validate UDP header checksum

                                let reply = Packet::decode(udp_data)?;

                                warn!("DHCP packet decoded");

                                if let Some((mt, settings)) = reply.parse_reply(&self.mac, xid) {
                                    if expected_message_types.iter().any(|emt| mt == *emt) {
                                        return Ok(Some((mt, settings)));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    return Ok(None);
                }
            }

            Err(Error::Timeout)
        }
    }
}

pub mod server {
    use core::fmt::Debug;

    use embassy_time::{Duration, Instant};
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
        mac: [u8; 6],
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
                .bind_multiple(SocketAddr::new(IpAddr::V4(self.ip), 66))
                .await
                .map_err(Error::Io)?;

            loop {
                self.handle::<U>(&mut socket, buf).await?;
            }
        }

        async fn handle<U: UdpStack>(
            &mut self,
            socket: &mut U::MultiplyBound,
            buf: &mut [u8],
        ) -> Result<(), Error<U::Error>> {
            let (len, local_addr, remote_addr) =
                socket.receive_into(buf).await.map_err(Error::Io)?;

            let request = Packet::decode(&buf[..len])?;

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

                            socket
                                .send(local_addr, remote_addr, data)
                                .await
                                .map_err(Error::Io)?;
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

        fn is_available(&self, mac: &[u8; 6], addr: Ipv4Addr) -> bool {
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

        fn current_lease(&self, mac: &[u8; 6]) -> Option<Ipv4Addr> {
            self.leases
                .iter()
                .find_map(|(addr, lease)| (lease.mac == *mac).then_some(*addr))
        }

        fn add_lease(&mut self, addr: Ipv4Addr, mac: [u8; 6], expires: Instant) -> bool {
            self.remove_lease(&mac);

            self.leases.insert(addr, Lease { mac, expires }).is_ok()
        }

        fn remove_lease(&mut self, mac: &[u8; 6]) -> bool {
            if let Some(addr) = self.current_lease(mac) {
                self.leases.remove(&addr);

                true
            } else {
                false
            }
        }
    }
}
