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

    use embedded_io_async::{Read, Write};

    use embedded_nal_async::Ipv4Addr;

    use log::{info, trace};

    use rand_core::RngCore;

    use self::dhcp::{MessageType, Options, Packet};

    pub use super::*;
    pub use crate::dhcp::Settings;

    use crate::dhcp::raw_ip::{Ipv4PacketHeader, UdpPacketHeader};

    #[derive(Clone, Debug)]
    pub struct Configuration {
        pub mac: [u8; 6],
        pub raw_packets: bool,
        pub client_port: Option<u16>,
        pub server_port: u16,
        pub timeout: Duration,
    }

    impl Configuration {
        pub const fn new(mac: [u8; 6]) -> Self {
            Self {
                mac,
                raw_packets: true,
                client_port: Some(68),
                server_port: 67,
                timeout: Duration::from_secs(10),
            }
        }
    }

    pub struct Client<T> {
        rng: T,
        mac: [u8; 6],
        raw_packets: bool,
        client_port: Option<u16>,
        server_port: u16,
        timeout: Duration,
    }

    impl<T> Client<T>
    where
        T: RngCore,
    {
        pub fn new(rng: T, conf: &Configuration) -> Self {
            info!("Starting DHCP client with configuration {conf:?}");

            Self {
                rng,
                mac: conf.mac,
                raw_packets: conf.raw_packets,
                client_port: conf.client_port,
                server_port: conf.server_port,
                timeout: conf.timeout,
            }
        }

        pub async fn discover<W: Write>(
            &mut self,
            write: W,
            buf: &mut [u8],
            secs: u16,
            ip: Option<Ipv4Addr>,
        ) -> Result<u32, Error<W::Error>> {
            let mut opt_buf = Options::buf();

            let xid = self.rng.next_u32();

            self.send(
                write,
                buf,
                secs,
                xid,
                None,
                None,
                Options::discover(ip, &mut opt_buf),
            )
            .await?;

            Ok(xid)
        }

        pub async fn request<W: Write>(
            &mut self,
            write: W,
            buf: &mut [u8],
            secs: u16,
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<u32, Error<W::Error>> {
            let mut opt_buf = Options::buf();

            let xid = self.rng.next_u32();

            self.send(
                write,
                buf,
                secs,
                xid,
                Some(server_ip),
                None,
                Options::request(our_ip, &mut opt_buf),
            )
            .await?;

            Ok(xid)
        }

        pub async fn release<W: Write>(
            &mut self,
            write: W,
            buf: &mut [u8],
            secs: u16,
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<(), Error<W::Error>> {
            let mut opt_buf = Options::buf();

            let xid = self.rng.next_u32();

            self.send(
                write,
                buf,
                secs,
                xid,
                Some(server_ip),
                Some(our_ip),
                Options::release(&mut opt_buf),
            )
            .await?;

            Ok(())
        }

        pub async fn decline<W: Write>(
            &mut self,
            write: W,
            buf: &mut [u8],
            secs: u16,
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<(), Error<W::Error>> {
            let mut opt_buf = Options::buf();

            let xid = self.rng.next_u32();

            self.send(
                write,
                buf,
                secs,
                xid,
                Some(server_ip),
                Some(our_ip),
                Options::decline(&mut opt_buf),
            )
            .await?;

            Ok(())
        }

        pub async fn wait_reply<'o, R: Read>(
            &self,
            read: R,
            buf: &'o mut [u8],
            xid: u32,
            expected_message_types: Option<&[MessageType]>,
        ) -> Result<Packet<'o>, Error<R::Error>> {
            self.recv(read, buf, xid, expected_message_types).await
        }

        async fn send<W: Write>(
            &self,
            mut write: W,
            buf: &mut [u8],
            secs: u16,
            xid: u32,
            server_ip: Option<Ipv4Addr>,
            our_ip: Option<Ipv4Addr>,
            options: Options<'_>,
            //expected_message_types: &[MessageType],
        ) -> Result<(), Error<W::Error>> {
            let packet = if self.raw_packets {
                if buf.len() < Ipv4PacketHeader::MIN_SIZE + UdpPacketHeader::SIZE {
                    Err(Error::Format(dhcp::Error::BufferOverflow))?;
                }

                let mut ip_hdr = Ipv4PacketHeader::new(
                    our_ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
                    server_ip.unwrap_or(Ipv4Addr::BROADCAST),
                    UdpPacketHeader::PROTO,
                );

                ip_hdr.encode_with_payload(buf, |buf, ip_hdr| {
                    let mut udp_hdr = UdpPacketHeader::new(68, self.server_port);

                    let len = udp_hdr
                        .encode_with_payload(buf, ip_hdr, |buf| {
                            let request =
                                Packet::new_request(self.mac, xid, secs, our_ip, options.clone());

                            let len = request.encode(buf)?.len();

                            Ok(len)
                        })?
                        .len();

                    Ok(len)
                })?
            } else {
                let request = Packet::new_request(self.mac, xid, secs, our_ip, options.clone());

                request.encode(buf)?
            };

            write.write_all(packet).await.map_err(Error::Io)?;

            Ok(())
        }

        async fn recv<'o, R: Read>(
            &self,
            mut read: R,
            buf: &'o mut [u8],
            xid: u32,
            expected_message_types: Option<&[MessageType]>,
        ) -> Result<Packet<'o>, Error<R::Error>> {
            trace!("Awaiting response packet");

            let start = Instant::now();
            let mut now = start;

            let reply = loop {
                let timer = Timer::after(if now < start + self.timeout {
                    start + self.timeout - now
                } else {
                    Duration::from_secs(1)
                });

                // NLL...
                let buf = unsafe { (buf as *mut [u8]).as_mut().unwrap() };

                let len = match select(read.read(buf), timer).await {
                    Either::First(result) => result.map_err(Error::Io)?,
                    Either::Second(_) => Err(Error::Timeout)?,
                };

                let packet = &buf[..len];

                let decode = |packet| {
                    let reply = Packet::decode(packet)?;

                    info!("DHCP packet decoded:\n{reply:?}");

                    if reply.is_for_us(&self.mac, xid) {
                        if let Some(expected_message_types) = expected_message_types {
                            let (mt, _) = reply.settings().unwrap();

                            if expected_message_types.iter().any(|emt| mt == *emt) {
                                return Ok(Some(reply));
                            }
                        } else {
                            return Ok(Some(reply));
                        }
                    }

                    Ok::<_, dhcp::Error>(None)
                };

                if self.raw_packets {
                    if let Some((ip_hdr, ip_payload)) =
                        Ipv4PacketHeader::decode_with_payload(packet)?
                    {
                        if ip_hdr.p == UdpPacketHeader::PROTO {
                            let (udp_hdr, udp_payload) =
                                UdpPacketHeader::decode_with_payload(ip_payload, &ip_hdr)?;

                            if udp_hdr.src == self.server_port
                                && self
                                    .client_port
                                    .map(|port| port == udp_hdr.dst)
                                    .unwrap_or(true)
                            {
                                if let Some(reply) = decode(udp_payload)? {
                                    break reply;
                                }
                            }
                        }
                    }
                } else {
                    if let Some(reply) = decode(packet)? {
                        break reply;
                    }
                }

                now = Instant::now();
            };

            Ok(reply)
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
