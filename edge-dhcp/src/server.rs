use core::fmt::Debug;

use embassy_time::{Duration, Instant};

use log::info;

use super::*;

#[derive(Clone, Debug)]
pub struct Lease {
    mac: [u8; 16],
    expires: Instant,
}

#[derive(Clone, Debug)]
pub enum Action<'a> {
    Discover(Option<Ipv4Addr>, &'a [u8; 16]),
    Request(Ipv4Addr, &'a [u8; 16]),
    Release(Ipv4Addr, &'a [u8; 16]),
    Decline(Ipv4Addr, &'a [u8; 16]),
}

pub struct ServerOptions<'a> {
    pub ip: Ipv4Addr,
    pub gateways: &'a [Ipv4Addr],
    pub subnet: Option<Ipv4Addr>,
    pub dns: &'a [Ipv4Addr],
    pub lease_duration: Duration,
}

impl<'a> ServerOptions<'a> {
    pub fn process<'o>(&self, request: &'o Packet<'o>) -> Option<Action<'o>> {
        if request.reply {
            return None;
        }

        let message_type = request.options.iter().find_map(|option| {
            if let DhcpOption::MessageType(message_type) = option {
                Some(message_type)
            } else {
                None
            }
        });

        let message_type = if let Some(message_type) = message_type {
            message_type
        } else {
            info!("Ignoring DHCP request, no message type found: {request:?}");
            return None;
        };

        let server_identifier = request.options.iter().find_map(|option| {
            if let DhcpOption::ServerIdentifier(ip) = option {
                Some(ip)
            } else {
                None
            }
        });

        if !(server_identifier == Some(self.ip)
            || server_identifier.is_none() && matches!(message_type, MessageType::Discover))
        {
            info!("Ignoring {message_type} request, not addressed to this server: {request:?}");
            return None;
        }

        info!("Received {message_type} request: {request:?}");
        match message_type {
            MessageType::Discover => {
                let requested_ip = request.options.iter().find_map(|option| {
                    if let DhcpOption::RequestedIpAddress(ip) = option {
                        Some(ip)
                    } else {
                        None
                    }
                });

                Some(Action::Discover(requested_ip, &request.chaddr))
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

                Some(Action::Request(ip, &request.chaddr))
            }
            MessageType::Release => Some(Action::Release(request.yiaddr, &request.chaddr)),
            MessageType::Decline => Some(Action::Decline(request.yiaddr, &request.chaddr)),
            _ => None,
        }
    }

    pub fn offer(
        &self,
        request: &Packet,
        yiaddr: Ipv4Addr,
        opt_buf: &'a mut [DhcpOption<'a>],
    ) -> Packet<'a> {
        self.reply(request, MessageType::Offer, Some(yiaddr), opt_buf)
    }

    pub fn ack_nak(
        &self,
        request: &Packet,
        ip: Option<Ipv4Addr>,
        opt_buf: &'a mut [DhcpOption<'a>],
    ) -> Packet<'a> {
        if let Some(ip) = ip {
            self.reply(request, MessageType::Ack, Some(ip), opt_buf)
        } else {
            self.reply(request, MessageType::Nak, None, opt_buf)
        }
    }

    fn reply(
        &self,
        request: &Packet,
        message_type: MessageType,
        ip: Option<Ipv4Addr>,
        buf: &'a mut [DhcpOption<'a>],
    ) -> Packet<'a> {
        let reply = request.new_reply(
            ip,
            request.options.reply(
                message_type,
                self.ip,
                self.lease_duration.as_secs() as _,
                self.gateways,
                self.subnet,
                self.dns,
                buf,
            ),
        );

        info!("Sending {message_type} reply: {reply:?}");

        reply
    }
}

/// A simple DHCP server.
/// The server is unaware of the IP/UDP transport layer and operates purely in terms of packets
/// represented as Rust slices.
#[derive(Clone, Debug)]
pub struct Server<const N: usize> {
    pub range_start: Ipv4Addr,
    pub range_end: Ipv4Addr,
    pub leases: heapless::LinearMap<Ipv4Addr, Lease, N>,
}

impl<const N: usize> Server<N> {
    pub fn handle_request<'o>(
        &mut self,
        opt_buf: &'o mut [DhcpOption<'o>],
        server_options: &'o ServerOptions,
        request: &Packet,
    ) -> Option<Packet<'o>> {
        server_options
            .process(request)
            .and_then(|action| match action {
                Action::Discover(requested_ip, mac) => {
                    let ip = requested_ip
                        .and_then(|ip| self.is_available(mac, ip).then_some(ip))
                        .or_else(|| self.current_lease(mac))
                        .or_else(|| self.available());

                    ip.map(|ip| server_options.offer(request, ip, opt_buf))
                }
                Action::Request(ip, mac) => {
                    let ip = (self.is_available(mac, ip)
                        && self.add_lease(
                            ip,
                            request.chaddr,
                            Instant::now() + server_options.lease_duration,
                        ))
                    .then_some(ip);

                    Some(server_options.ack_nak(request, ip, opt_buf))
                }
                Action::Release(_ip, mac) | Action::Decline(_ip, mac) => {
                    self.remove_lease(mac);

                    None
                }
            })
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
