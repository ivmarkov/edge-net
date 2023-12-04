#![cfg_attr(not(feature = "std"), no_std)]
#![allow(stable_features)]
#![allow(unknown_lints)]
#![feature(async_fn_in_trait)]
#![allow(async_fn_in_trait)]
#![feature(impl_trait_projections)]

/// This code is a `no_std` and no-alloc modification of https://github.com/krolaw/dhcp4r
use core::str::Utf8Error;

use no_std_net::Ipv4Addr;

use num_enum::TryFromPrimitive;

#[cfg(feature = "nightly")]
pub mod asynch;

use self::raw_ip::{Ipv4PacketHeader, UdpPacketHeader};

#[derive(Debug)]
pub enum Error {
    DataUnderflow,
    InvalidUtf8Str(Utf8Error),
    InvalidMessageType,
    MissingCookie,
    InvalidHlen,
    BufferOverflow,
    InvalidPacket,
}

///
/// DHCP Message Type.
///
/// # Standards
///
/// The semantics of the various DHCP message types are described in RFC 2131 (see Table 2).
/// Their numeric values are described in Section 9.6 of RFC 2132, which begins:
///
/// > This option is used to convey the type of the DHCP message.  The code for this option is 53,
/// > and its length is 1.
///
#[derive(Copy, Clone, PartialEq, Eq, Debug, TryFromPrimitive)]
#[repr(u8)]
pub enum MessageType {
    /// Client broadcast to locate available servers.
    Discover = 1,

    /// Server to client in response to DHCPDISCOVER with offer of configuration parameters.
    Offer = 2,

    /// Client message to servers either (a) requesting offered parameters from one server and
    /// implicitly declining offers from all others, (b) confirming correctness of previously
    /// allocated address after, e.g., system reboot, or (c) extending the lease on a particular
    /// network address.
    Request = 3,

    /// Client to server indicating network address is already in use.
    Decline = 4,

    /// Server to client with configuration parameters, including committed network address.
    Ack = 5,

    /// Server to client indicating client's notion of network address is incorrect (e.g., client
    /// has moved to new subnet) or client's lease as expired.
    Nak = 6,

    /// Client to server relinquishing network address and cancelling remaining lease.
    Release = 7,

    /// Client to server, asking only for local configuration parameters; client already has
    /// externally configured network address.
    Inform = 8,
}

/// DHCP Packet Structure
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Packet<'a> {
    pub reply: bool,
    pub hops: u8,
    pub xid: u32,
    pub secs: u16,
    pub broadcast: bool,
    pub ciaddr: Ipv4Addr,
    pub yiaddr: Ipv4Addr,
    pub siaddr: Ipv4Addr,
    pub giaddr: Ipv4Addr,
    pub chaddr: [u8; 16],
    pub options: Options<'a>,
}

impl<'a> Packet<'a> {
    const COOKIE: [u8; 4] = [99, 130, 83, 99];

    const BOOT_REQUEST: u8 = 1; // From Client
    const BOOT_REPLY: u8 = 2; // From Server

    const SERVER_NAME_AND_FILE_NAME: usize = 64 + 128;

    const END: u8 = 255;
    const PAD: u8 = 0;

    pub fn new_request(
        mac: [u8; 6],
        xid: u32,
        secs: u16,
        our_ip: Option<Ipv4Addr>,
        options: Options<'a>,
    ) -> Self {
        let mut chaddr = [0; 16];
        chaddr[..6].copy_from_slice(&mac);

        Self {
            reply: false,
            hops: 0,
            xid,
            secs,
            broadcast: our_ip.is_none(),
            ciaddr: our_ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
            yiaddr: our_ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr,
            options,
        }
    }

    pub fn new_reply<'b>(&self, ip: Option<Ipv4Addr>, options: Options<'b>) -> Packet<'b> {
        Packet {
            reply: true,
            hops: 0,
            xid: self.xid,
            secs: 0,
            broadcast: self.broadcast,
            ciaddr: ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
            yiaddr: ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr: self.chaddr,
            options,
        }
    }

    pub fn is_for_us(&self, mac: &[u8; 6], xid: u32) -> bool {
        const MAC_TRAILING_ZEROS: [u8; 10] = [0; 10];

        self.chaddr[0..6] == *mac
            && self.chaddr[6..16] == MAC_TRAILING_ZEROS
            && self.xid == xid
            && self.reply
    }

    pub fn settings(&self) -> Option<(MessageType, Settings)> {
        if self.reply {
            let mt = self.options.iter().find_map(|option| {
                if let DhcpOption::MessageType(mt) = option {
                    Some(mt)
                } else {
                    None
                }
            });

            mt.map(|mt| (mt, self.into()))
        } else {
            None
        }
    }

    /// Parses the packet from a byte slice
    pub fn decode(data: &'a [u8]) -> Result<Self, Error> {
        let mut bytes = BytesIn::new(data);

        Ok(Self {
            reply: {
                let reply = bytes.byte()? == Self::BOOT_REPLY;
                let _htype = bytes.byte()?; // Hardware address type; 1 = 10Mb Ethernet
                let hlen = bytes.byte()?;

                if hlen != 6 {
                    Err(Error::InvalidHlen)?;
                }

                reply
            },
            hops: bytes.byte()?,
            xid: u32::from_be_bytes(bytes.arr()?),
            secs: u16::from_be_bytes(bytes.arr()?),
            broadcast: u16::from_be_bytes(bytes.arr()?) & 128 != 0,
            ciaddr: bytes.arr()?.into(),
            yiaddr: bytes.arr()?.into(),
            siaddr: bytes.arr()?.into(),
            giaddr: bytes.arr()?.into(),
            chaddr: bytes.arr()?,
            options: {
                for _ in 0..Self::SERVER_NAME_AND_FILE_NAME {
                    bytes.byte()?;
                }

                if bytes.arr()? != Self::COOKIE {
                    Err(Error::MissingCookie)?;
                }

                Options(OptionsInner::decode(bytes.remaining())?)
            },
        })
    }

    /// Encodes the packet into the provided buf slice
    pub fn encode<'o>(&self, buf: &'o mut [u8]) -> Result<&'o [u8], Error> {
        let mut bytes = BytesOut::new(buf);

        bytes
            .push(&[if self.reply {
                Self::BOOT_REPLY
            } else {
                Self::BOOT_REQUEST
            }])?
            .byte(1)?
            .byte(6)?
            .byte(self.hops)?
            .push(&u32::to_be_bytes(self.xid))?
            .push(&u16::to_be_bytes(self.secs))?
            .push(&u16::to_be_bytes(if self.broadcast { 128 } else { 0 }))?
            .push(&self.ciaddr.octets())?
            .push(&self.yiaddr.octets())?
            .push(&self.siaddr.octets())?
            .push(&self.giaddr.octets())?
            .push(&self.chaddr)?;

        for _ in 0..Self::SERVER_NAME_AND_FILE_NAME {
            bytes.byte(0)?;
        }

        bytes.push(&Self::COOKIE)?;

        self.options.0.encode(&mut bytes)?;

        bytes.byte(Self::END)?;

        while bytes.len() < 272 {
            bytes.byte(Self::PAD)?;
        }

        let len = bytes.len();

        Ok(&buf[..len])
    }

    /// Parses the packet from a byte slice that models a raw IP packet
    /// Useful when working with raw sockets
    pub fn decode_raw(
        data: &'a [u8],
        src_port: Option<u16>,
        dst_port: Option<u16>,
    ) -> Result<Option<(Ipv4PacketHeader, UdpPacketHeader, Self)>, Error> {
        if let Some((ip_hdr, ip_payload)) = Ipv4PacketHeader::decode_with_payload(data)? {
            if ip_hdr.p == UdpPacketHeader::PROTO {
                let (udp_hdr, udp_payload) =
                    UdpPacketHeader::decode_with_payload(ip_payload, &ip_hdr)?;

                if src_port.map(|p| p == udp_hdr.src).unwrap_or(true)
                    && dst_port.map(|p| p == udp_hdr.dst).unwrap_or(true)
                {
                    return Ok(Some((ip_hdr, udp_hdr, Packet::decode(udp_payload)?)));
                }
            }
        }

        Ok(None)
    }

    /// Encodes the packet into the provided buf slice, together with a UDP and IPv$ headers
    /// Useful when working with raw sockets
    pub fn encode_raw<'o>(
        &self,
        src_ip: Option<Ipv4Addr>,
        src_port: u16,
        dst_ip: Option<Ipv4Addr>,
        dst_port: u16,
        buf: &'o mut [u8],
    ) -> Result<&'o [u8], Error> {
        if buf.len() < Ipv4PacketHeader::MIN_SIZE + UdpPacketHeader::SIZE {
            Err(Error::BufferOverflow)?;
        }

        let mut ip_hdr = Ipv4PacketHeader::new(
            src_ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
            dst_ip.unwrap_or(Ipv4Addr::BROADCAST),
            UdpPacketHeader::PROTO,
        );

        ip_hdr.encode_with_payload(buf, |buf, ip_hdr| {
            let mut udp_hdr = UdpPacketHeader::new(src_port, dst_port);

            let len = udp_hdr
                .encode_with_payload(buf, ip_hdr, |buf| {
                    let len = self.encode(buf)?.len();

                    Ok(len)
                })?
                .len();

            Ok(len)
        })
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub ip: Ipv4Addr,
    pub server_ip: Option<Ipv4Addr>,
    pub lease_time_secs: Option<u32>,
    pub gateway: Option<Ipv4Addr>,
    pub subnet: Option<Ipv4Addr>,
    pub dns1: Option<Ipv4Addr>,
    pub dns2: Option<Ipv4Addr>,
}

impl From<&Packet<'_>> for Settings {
    fn from(packet: &Packet) -> Self {
        Self {
            ip: packet.yiaddr,
            server_ip: packet.options.iter().find_map(|option| {
                if let DhcpOption::ServerIdentifier(ip) = option {
                    Some(ip)
                } else {
                    None
                }
            }),
            lease_time_secs: packet.options.iter().find_map(|option| {
                if let DhcpOption::IpAddressLeaseTime(lease_time_secs) = option {
                    Some(lease_time_secs)
                } else {
                    None
                }
            }),
            gateway: packet.options.iter().find_map(|option| {
                if let DhcpOption::Router(ips) = option {
                    ips.iter().next()
                } else {
                    None
                }
            }),
            subnet: packet.options.iter().find_map(|option| {
                if let DhcpOption::SubnetMask(subnet) = option {
                    Some(subnet)
                } else {
                    None
                }
            }),
            dns1: packet.options.iter().find_map(|option| {
                if let DhcpOption::DomainNameServer(ips) = option {
                    ips.iter().next()
                } else {
                    None
                }
            }),
            dns2: packet.options.iter().find_map(|option| {
                if let DhcpOption::DomainNameServer(ips) = option {
                    ips.iter().nth(1)
                } else {
                    None
                }
            }),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Options<'a>(OptionsInner<'a>);

impl<'a> Options<'a> {
    const REQUEST_PARAMS: &'static [u8] = &[
        DhcpOption::CODE_ROUTER,
        DhcpOption::CODE_SUBNET,
        DhcpOption::CODE_DNS,
    ];

    pub const fn new(options: &'a [DhcpOption<'a>]) -> Self {
        Self(OptionsInner::DataSlice(options))
    }

    #[inline(always)]
    pub const fn buf() -> [DhcpOption<'a>; 8] {
        [DhcpOption::Message(""); 8]
    }

    pub fn discover(requested_ip: Option<Ipv4Addr>, buf: &'a mut [DhcpOption<'a>]) -> Self {
        buf[0] = DhcpOption::MessageType(MessageType::Discover);

        let mut offset = 1;

        if let Some(requested_ip) = requested_ip {
            buf[1] = DhcpOption::RequestedIpAddress(requested_ip);
            offset += 1;
        }

        Self::new(&buf[..offset])
    }

    pub fn request(ip: Ipv4Addr, buf: &'a mut [DhcpOption<'a>]) -> Self {
        buf[0] = DhcpOption::MessageType(MessageType::Request);
        buf[1] = DhcpOption::RequestedIpAddress(ip);
        buf[2] = DhcpOption::ParameterRequestList(Self::REQUEST_PARAMS);

        Self::new(&buf[..3])
    }

    pub fn release(buf: &'a mut [DhcpOption<'a>]) -> Self {
        buf[0] = DhcpOption::MessageType(MessageType::Release);

        Self::new(&buf[..1])
    }

    pub fn decline(buf: &'a mut [DhcpOption<'a>]) -> Self {
        buf[0] = DhcpOption::MessageType(MessageType::Decline);

        Self::new(&buf[..1])
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reply<'b>(
        &self,
        mt: MessageType,
        server_ip: Ipv4Addr,
        lease_duration_secs: u32,
        gateways: &'b [Ipv4Addr],
        subnet: Option<Ipv4Addr>,
        dns: &'b [Ipv4Addr],
        buf: &'b mut [DhcpOption<'b>],
    ) -> Options<'b> {
        let requested = self.iter().find_map(|option| {
            if let DhcpOption::ParameterRequestList(requested) = option {
                Some(requested)
            } else {
                None
            }
        });

        Options::internal_reply(
            requested,
            mt,
            server_ip,
            lease_duration_secs,
            gateways,
            subnet,
            dns,
            buf,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn internal_reply(
        requested: Option<&[u8]>,
        mt: MessageType,
        server_ip: Ipv4Addr,
        lease_duration_secs: u32,
        gateways: &'a [Ipv4Addr],
        subnet: Option<Ipv4Addr>,
        dns: &'a [Ipv4Addr],
        buf: &'a mut [DhcpOption<'a>],
    ) -> Self {
        buf[0] = DhcpOption::MessageType(mt);
        buf[1] = DhcpOption::ServerIdentifier(server_ip);
        buf[2] = DhcpOption::IpAddressLeaseTime(lease_duration_secs);

        let mut offset = 3;

        if !matches!(mt, MessageType::Nak) {
            if let Some(requested) = requested {
                for code in requested {
                    if !buf[0..offset].iter().any(|option| option.code() == *code) {
                        let option = match *code {
                            DhcpOption::CODE_ROUTER => (!gateways.is_empty())
                                .then_some(DhcpOption::Router(Ipv4Addrs::new(gateways))),
                            DhcpOption::CODE_DNS => (!dns.is_empty())
                                .then_some(DhcpOption::DomainNameServer(Ipv4Addrs::new(dns))),
                            DhcpOption::CODE_SUBNET => subnet.map(DhcpOption::SubnetMask),
                            _ => None,
                        };

                        if let Some(option) = option {
                            buf[offset] = option;
                            offset += 1;
                        }
                    }

                    if offset == buf.len() {
                        break;
                    }
                }
            }
        }

        Self::new(&buf[..offset])
    }

    pub fn iter(&self) -> impl Iterator<Item = DhcpOption<'a>> + 'a {
        self.0.iter()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum OptionsInner<'a> {
    ByteSlice(&'a [u8]),
    DataSlice(&'a [DhcpOption<'a>]),
}

impl<'a> OptionsInner<'a> {
    fn decode(data: &'a [u8]) -> Result<Self, Error> {
        let mut bytes = BytesIn::new(data);

        while DhcpOption::decode(&mut bytes)?.is_some() {}

        Ok(Self::ByteSlice(data))
    }

    fn encode(&self, buf: &mut BytesOut) -> Result<(), Error> {
        for option in self.iter() {
            option.encode(buf)?;
        }

        Ok(())
    }

    fn iter(&self) -> impl Iterator<Item = DhcpOption<'a>> + 'a {
        struct ByteSliceDhcpOptions<'a>(BytesIn<'a>);

        impl<'a> Iterator for ByteSliceDhcpOptions<'a> {
            type Item = DhcpOption<'a>;

            fn next(&mut self) -> Option<Self::Item> {
                if self.0.is_empty() {
                    None
                } else {
                    DhcpOption::decode(&mut self.0).unwrap()
                }
            }
        }

        match self {
            Self::ByteSlice(data) => {
                EitherIterator::First(ByteSliceDhcpOptions(BytesIn::new(data)))
            }
            Self::DataSlice(data) => EitherIterator::Second(data.iter().cloned()),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DhcpOption<'a> {
    MessageType(MessageType),
    ServerIdentifier(Ipv4Addr),
    ParameterRequestList(&'a [u8]),
    RequestedIpAddress(Ipv4Addr),
    HostName(&'a str),
    Router(Ipv4Addrs<'a>),
    DomainNameServer(Ipv4Addrs<'a>),
    IpAddressLeaseTime(u32),
    SubnetMask(Ipv4Addr),
    Message(&'a str),
    Unrecognized(u8, &'a [u8]),
}

impl<'a> DhcpOption<'a> {
    pub const CODE_ROUTER: u8 = DhcpOption::Router(Ipv4Addrs::new(&[])).code();
    pub const CODE_DNS: u8 = DhcpOption::DomainNameServer(Ipv4Addrs::new(&[])).code();
    pub const CODE_SUBNET: u8 = DhcpOption::SubnetMask(Ipv4Addr::new(0, 0, 0, 0)).code();

    fn decode<'o>(bytes: &mut BytesIn<'o>) -> Result<Option<DhcpOption<'o>>, Error> {
        let code = bytes.byte()?;
        if code == Packet::END {
            Ok(None)
        } else {
            let len = bytes.byte()? as usize;
            let mut bytes = BytesIn::new(bytes.slice(len)?);

            let option = match code {
                DHCP_MESSAGE_TYPE => DhcpOption::MessageType(
                    TryFromPrimitive::try_from_primitive(bytes.remaining_byte()?)
                        .map_err(|_| Error::InvalidMessageType)?,
                ),
                SERVER_IDENTIFIER => {
                    DhcpOption::ServerIdentifier(Ipv4Addr::from(bytes.remaining_arr()?))
                }
                PARAMETER_REQUEST_LIST => DhcpOption::ParameterRequestList(bytes.remaining()),
                REQUESTED_IP_ADDRESS => {
                    DhcpOption::RequestedIpAddress(Ipv4Addr::from(bytes.remaining_arr()?))
                }
                HOST_NAME => DhcpOption::HostName(
                    core::str::from_utf8(bytes.remaining()).map_err(Error::InvalidUtf8Str)?,
                ),
                ROUTER => {
                    DhcpOption::Router(Ipv4Addrs(Ipv4AddrsInner::ByteSlice(bytes.remaining())))
                }
                DOMAIN_NAME_SERVER => DhcpOption::DomainNameServer(Ipv4Addrs(
                    Ipv4AddrsInner::ByteSlice(bytes.remaining()),
                )),
                IP_ADDRESS_LEASE_TIME => {
                    DhcpOption::IpAddressLeaseTime(u32::from_be_bytes(bytes.remaining_arr()?))
                }
                SUBNET_MASK => DhcpOption::SubnetMask(Ipv4Addr::from(bytes.remaining_arr()?)),
                MESSAGE => DhcpOption::Message(
                    core::str::from_utf8(bytes.remaining()).map_err(Error::InvalidUtf8Str)?,
                ),
                _ => DhcpOption::Unrecognized(code, bytes.remaining()),
            };

            Ok(Some(option))
        }
    }

    fn encode(&self, out: &mut BytesOut) -> Result<(), Error> {
        out.byte(self.code())?;

        self.data(|data| {
            out.byte(data.len() as _)?;
            out.push(data)?;

            Ok(())
        })
    }

    pub const fn code(&self) -> u8 {
        match self {
            Self::MessageType(_) => DHCP_MESSAGE_TYPE,
            Self::ServerIdentifier(_) => SERVER_IDENTIFIER,
            Self::ParameterRequestList(_) => PARAMETER_REQUEST_LIST,
            Self::RequestedIpAddress(_) => REQUESTED_IP_ADDRESS,
            Self::HostName(_) => HOST_NAME,
            Self::Router(_) => ROUTER,
            Self::DomainNameServer(_) => DOMAIN_NAME_SERVER,
            Self::IpAddressLeaseTime(_) => IP_ADDRESS_LEASE_TIME,
            Self::SubnetMask(_) => SUBNET_MASK,
            Self::Message(_) => MESSAGE,
            Self::Unrecognized(code, _) => *code,
        }
    }

    fn data(&self, mut f: impl FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        match self {
            Self::MessageType(mtype) => f(&[*mtype as _]),
            Self::ServerIdentifier(addr) => f(&addr.octets()),
            Self::ParameterRequestList(prl) => f(prl),
            Self::RequestedIpAddress(addr) => f(&addr.octets()),
            Self::HostName(name) => f(name.as_bytes()),
            Self::Router(addrs) | Self::DomainNameServer(addrs) => {
                for addr in addrs.iter() {
                    f(&addr.octets())?;
                }

                Ok(())
            }
            Self::IpAddressLeaseTime(secs) => f(&secs.to_be_bytes()),
            Self::SubnetMask(mask) => f(&mask.octets()),
            Self::Message(msg) => f(msg.as_bytes()),
            Self::Unrecognized(_, data) => f(data),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Ipv4Addrs<'a>(Ipv4AddrsInner<'a>);

impl<'a> Ipv4Addrs<'a> {
    pub const fn new(addrs: &'a [Ipv4Addr]) -> Self {
        Self(Ipv4AddrsInner::DataSlice(addrs))
    }

    pub fn iter(&self) -> impl Iterator<Item = Ipv4Addr> + 'a {
        self.0.iter()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Ipv4AddrsInner<'a> {
    ByteSlice(&'a [u8]),
    DataSlice(&'a [Ipv4Addr]),
}

impl<'a> Ipv4AddrsInner<'a> {
    fn iter(&self) -> impl Iterator<Item = Ipv4Addr> + 'a {
        match self {
            Self::ByteSlice(data) => {
                EitherIterator::First((0..data.len()).step_by(4).map(|offset| {
                    let octets: [u8; 4] = data[offset..offset + 4].try_into().unwrap();

                    octets.into()
                }))
            }
            Self::DataSlice(data) => EitherIterator::Second(data.iter().cloned()),
        }
    }
}

enum EitherIterator<F, S> {
    First(F),
    Second(S),
}

impl<F, S> Iterator for EitherIterator<F, S>
where
    F: Iterator,
    S: Iterator<Item = F::Item>,
{
    type Item = F::Item;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::First(iter) => iter.next(),
            Self::Second(iter) => iter.next(),
        }
    }
}

struct BytesIn<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> BytesIn<'a> {
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.offset == self.data.len()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn byte(&mut self) -> Result<u8, Error> {
        self.arr::<1>().map(|arr| arr[0])
    }

    pub fn slice(&mut self, len: usize) -> Result<&'a [u8], Error> {
        if len > self.data.len() - self.offset {
            Err(Error::DataUnderflow)
        } else {
            let data = &self.data[self.offset..self.offset + len];
            self.offset += len;

            Ok(data)
        }
    }

    pub fn arr<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let slice = self.slice(N)?;

        let mut data = [0; N];
        data.copy_from_slice(slice);

        Ok(data)
    }

    pub fn remaining(&mut self) -> &'a [u8] {
        let data = self.slice(self.data.len() - self.offset).unwrap();

        self.offset = self.data.len();

        data
    }

    pub fn remaining_byte(&mut self) -> Result<u8, Error> {
        Ok(self.remaining_arr::<1>()?[0])
    }

    pub fn remaining_arr<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        if self.data.len() - self.offset > N {
            Err(Error::InvalidHlen) // TODO
        } else {
            self.arr::<N>()
        }
    }
}

struct BytesOut<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> BytesOut<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    pub fn len(&self) -> usize {
        self.offset
    }

    pub fn byte(&mut self, data: u8) -> Result<&mut Self, Error> {
        self.push(&[data])
    }

    pub fn push(&mut self, data: &[u8]) -> Result<&mut Self, Error> {
        if data.len() > self.buf.len() - self.offset {
            Err(Error::BufferOverflow)
        } else {
            self.buf[self.offset..self.offset + data.len()].copy_from_slice(data);
            self.offset += data.len();

            Ok(self)
        }
    }
}

pub mod client {
    use log::trace;

    use rand_core::RngCore;

    use super::*;

    /// A simple DHCP client.
    /// The client is unaware of the IP/UDP transport layer and operates purely in terms of packets
    /// represented as Rust slices.
    ///
    /// As such, the client can generate all BOOTP requests and parse BOOTP replies.
    ///
    /// The client supports both raw IP as well as regular UDP payloads, where the raw payloads are
    /// automatically prefixed/unprefixed with the IP and UDP header, which allows this client to be used with a raw sockets' transport layer.
    ///
    /// Note that it is unlikely that a non-raw socket transport would actually even work, due to the peculiarities of the
    /// DHCP protocol, where a lot of UDP packets are send (and often broadcasted) by the client before the client actually has an assigned IP.
    pub struct Client<T> {
        pub rng: T,
        pub mac: [u8; 6],
        pub rp_udp_client_port: Option<u16>,
        pub rp_udp_server_port: Option<u16>,
    }

    impl<T> Client<T>
    where
        T: RngCore,
    {
        pub fn encode_discover<'o>(
            &mut self,
            buf: &'o mut [u8],
            secs: u16,
            ip: Option<Ipv4Addr>,
        ) -> Result<(&'o [u8], u32), Error> {
            let mut opt_buf = Options::buf();

            self.encode_bootp_request(buf, secs, None, None, Options::discover(ip, &mut opt_buf))
        }

        pub fn encode_request<'o>(
            &mut self,
            buf: &'o mut [u8],
            secs: u16,
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<(&'o [u8], u32), Error> {
            let mut opt_buf = Options::buf();

            self.encode_bootp_request(
                buf,
                secs,
                Some(server_ip),
                None,
                Options::request(our_ip, &mut opt_buf),
            )
        }

        pub fn encode_release<'o>(
            &mut self,
            buf: &'o mut [u8],
            secs: u16,
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<&'o [u8], Error> {
            let mut opt_buf = Options::buf();

            self.encode_bootp_request(
                buf,
                secs,
                Some(server_ip),
                Some(our_ip),
                Options::release(&mut opt_buf),
            )
            .map(|r| r.0)
        }

        pub fn encode_decline<'o>(
            &mut self,
            buf: &'o mut [u8],
            secs: u16,
            server_ip: Ipv4Addr,
            our_ip: Ipv4Addr,
        ) -> Result<&'o [u8], Error> {
            let mut opt_buf = Options::buf();

            self.encode_bootp_request(
                buf,
                secs,
                Some(server_ip),
                Some(our_ip),
                Options::decline(&mut opt_buf),
            )
            .map(|r| r.0)
        }

        #[allow(clippy::too_many_arguments)]
        pub fn encode_bootp_request<'o>(
            &mut self,
            buf: &'o mut [u8],
            secs: u16,
            server_ip: Option<Ipv4Addr>,
            our_ip: Option<Ipv4Addr>,
            options: Options<'_>,
        ) -> Result<(&'o [u8], u32), Error> {
            let xid = self.rng.next_u32();

            let request = Packet::new_request(self.mac, xid, secs, our_ip, options.clone());

            let data = if self.rp_udp_server_port.is_some() || self.rp_udp_client_port.is_some() {
                request.encode_raw(
                    our_ip,
                    self.rp_udp_client_port.unwrap_or(68),
                    server_ip,
                    self.rp_udp_server_port.unwrap_or(67),
                    buf,
                )?
            } else {
                request.encode(buf)?
            };

            Ok((data, xid))
        }

        pub fn decode_bootp_reply<'o>(
            &self,
            data: &'o [u8],
            xid: u32,
            expected_message_types: Option<&[MessageType]>,
        ) -> Result<Option<Packet<'o>>, Error> {
            let reply = if self.rp_udp_server_port.is_some() || self.rp_udp_client_port.is_some() {
                Packet::decode_raw(data, self.rp_udp_server_port, self.rp_udp_client_port)?
                    .map(|r| r.2)
            } else {
                Some(Packet::decode(data)?)
            };

            trace!("DHCP packet decoded:\n{reply:?}");

            Ok(reply.and_then(|reply| {
                if reply.is_for_us(&self.mac, xid) {
                    if let Some(expected_message_types) = expected_message_types {
                        let (mt, _) = reply.settings().unwrap();

                        if expected_message_types.iter().any(|emt| mt == *emt) {
                            return Some(reply);
                        }
                    } else {
                        return Some(reply);
                    }
                }

                None
            }))
        }
    }
}

pub mod server {
    use core::fmt::Debug;

    use embassy_time::{Duration, Instant};

    use log::{info, trace};

    use super::*;

    #[derive(Clone, Debug)]
    pub struct Lease {
        mac: [u8; 16],
        expires: Instant,
    }

    /// A simple DHCP server.
    /// The server is unaware of the IP/UDP transport layer and operates purely in terms of packets
    /// represented as Rust slices.
    ///
    /// The server supports both raw IP as well as regular UDP payloads, where the raw payloads are
    /// automatically prefixed/unprefixed with the IP and UDP header, which allows this server to be used with a raw sockets' transport layer.
    #[derive(Clone, Debug)]
    pub struct Server<const N: usize> {
        pub ip: Ipv4Addr,
        pub gateways: heapless::Vec<Ipv4Addr, 1>,
        pub subnet: Option<Ipv4Addr>,
        pub dns: heapless::Vec<Ipv4Addr, 2>,
        pub range_start: Ipv4Addr,
        pub range_end: Ipv4Addr,
        pub lease_duration: Duration,
        pub leases: heapless::LinearMap<Ipv4Addr, Lease, N>,
    }

    impl<const N: usize> Server<N> {
        pub fn handle_bootp_request<'o>(
            &mut self,
            rp_udp_server_port: Option<u16>,
            buf: &'o mut [u8],
            incoming_len: usize,
        ) -> Result<Option<&'o [u8]>, Error> {
            let request = if let Some(port) = rp_udp_server_port {
                Packet::decode_raw(&buf[..incoming_len], None, Some(port))?
                    .map(|(ip_hdr, udp_hdr, request)| (Some((ip_hdr, udp_hdr)), request))
            } else {
                Some((None, Packet::decode(&buf[..incoming_len])?))
            };

            if let Some((raw_hdrs, request)) = request {
                trace!("Got packet {request:?}");

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
                            info!("Packet is for us, will process, message type {mt:?}");

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
                                let packet = if let Some((ip_hdr, udp_hdr)) = raw_hdrs {
                                    reply.encode_raw(
                                        Some(self.ip),
                                        udp_hdr.dst,
                                        Some(ip_hdr.src),
                                        udp_hdr.src,
                                        buf,
                                    )?
                                } else {
                                    reply.encode(buf)?
                                };

                                return Ok(Some(packet));
                            }
                        }
                    }
                }
            }

            Ok(None)
        }

        fn reply_to<'a>(
            &'a self,
            request: &Packet<'_>,
            mt: MessageType,
            ip: Option<Ipv4Addr>,
            buf: &'a mut [DhcpOption<'a>],
        ) -> Packet<'a> {
            let reply = request.new_reply(
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
            );

            info!("Reply: {reply:?}");

            reply
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

// DHCP Options
const SUBNET_MASK: u8 = 1;
const ROUTER: u8 = 3;
const DOMAIN_NAME_SERVER: u8 = 6;
const HOST_NAME: u8 = 12;

// DHCP Extensions
const REQUESTED_IP_ADDRESS: u8 = 50;
const IP_ADDRESS_LEASE_TIME: u8 = 51;
const DHCP_MESSAGE_TYPE: u8 = 53;
const SERVER_IDENTIFIER: u8 = 54;
const PARAMETER_REQUEST_LIST: u8 = 55;
const MESSAGE: u8 = 56;

// IP and UDP headers as well as utility functions for (de)serializing those, as well as computing their checksums
//
// Useful in the context of DHCP, as it operates in terms of raw sockets (particuarly the client) so (dis)assembling
// IP & UDP packets "by hand" is necessary.
pub mod raw_ip {
    use log::trace;

    use no_std_net::Ipv4Addr;

    use super::{BytesIn, BytesOut, Error};

    #[derive(Clone, Debug)]
    pub struct Ipv4PacketHeader {
        pub version: u8,   // Version
        pub hlen: u8,      // Header length
        pub tos: u8,       // Type of service
        pub len: u16,      // Total length
        pub id: u16,       // Identification
        pub off: u16,      // Fragment offset field
        pub ttl: u8,       // Time to live
        pub p: u8,         // Protocol
        pub sum: u16,      // Checksum
        pub src: Ipv4Addr, // Source address
        pub dst: Ipv4Addr, // Dest address
    }

    impl Ipv4PacketHeader {
        pub const MIN_SIZE: usize = 20;
        pub const CHECKSUM_WORD: usize = 5;

        pub const IP_DF: u16 = 0x4000; // Don't fragment flag
        pub const IP_MF: u16 = 0x2000; // More fragments flag

        pub fn new(src: Ipv4Addr, dst: Ipv4Addr, proto: u8) -> Self {
            Self {
                version: 4,
                hlen: Self::MIN_SIZE as _,
                tos: 0,
                len: Self::MIN_SIZE as _,
                id: 0,
                off: 0,
                ttl: 64,
                p: proto,
                sum: 0,
                src,
                dst,
            }
        }

        /// Parses the packet from a byte slice
        pub fn decode(data: &[u8]) -> Result<Self, Error> {
            let mut bytes = BytesIn::new(data);

            let vhl = bytes.byte()?;

            Ok(Self {
                version: vhl >> 4,
                hlen: (vhl & 0x0f) * 4,
                tos: bytes.byte()?,
                len: u16::from_be_bytes(bytes.arr()?),
                id: u16::from_be_bytes(bytes.arr()?),
                off: u16::from_be_bytes(bytes.arr()?),
                ttl: bytes.byte()?,
                p: bytes.byte()?,
                sum: u16::from_be_bytes(bytes.arr()?),
                src: u32::from_be_bytes(bytes.arr()?).into(),
                dst: u32::from_be_bytes(bytes.arr()?).into(),
            })
        }

        /// Encodes the packet into the provided buf slice
        pub fn encode<'o>(&self, buf: &'o mut [u8]) -> Result<&'o [u8], Error> {
            let mut bytes = BytesOut::new(buf);

            bytes
                .byte(
                    (self.version << 4) | (self.hlen / 4 + (if self.hlen % 4 > 0 { 1 } else { 0 })),
                )?
                .byte(self.tos)?
                .push(&u16::to_be_bytes(self.len))?
                .push(&u16::to_be_bytes(self.id))?
                .push(&u16::to_be_bytes(self.off))?
                .byte(self.ttl)?
                .byte(self.p)?
                .push(&u16::to_be_bytes(self.sum))?
                .push(&u32::to_be_bytes(self.src.into()))?
                .push(&u32::to_be_bytes(self.dst.into()))?;

            let len = bytes.len();

            Ok(&buf[..len])
        }

        pub fn encode_with_payload<'o, F>(
            &mut self,
            buf: &'o mut [u8],
            encoder: F,
        ) -> Result<&'o [u8], Error>
        where
            F: FnOnce(&mut [u8], &Self) -> Result<usize, Error>,
        {
            let hdr_len = self.hlen as usize;
            if hdr_len < Self::MIN_SIZE || buf.len() < hdr_len {
                Err(Error::BufferOverflow)?;
            }

            let (hdr_buf, payload_buf) = buf.split_at_mut(hdr_len);

            let payload_len = encoder(payload_buf, self)?;

            let len = hdr_len + payload_len;
            self.len = len as _;

            let min_hdr_len = self.encode(hdr_buf)?.len();
            assert_eq!(min_hdr_len, Self::MIN_SIZE);

            hdr_buf[Self::MIN_SIZE..hdr_len].fill(0);

            let checksum = Self::checksum(hdr_buf);
            self.sum = checksum;

            Self::inject_checksum(hdr_buf, checksum);

            Ok(&buf[..len])
        }

        pub fn decode_with_payload(packet: &[u8]) -> Result<Option<(Self, &[u8])>, Error> {
            let hdr = Self::decode(packet)?;
            if hdr.version == 4 {
                // IPv4
                let len = hdr.len as usize;
                if packet.len() < len {
                    Err(Error::DataUnderflow)?;
                }

                let checksum = Self::checksum(&packet[..len]);

                trace!("IP header decoded, total_size={}, src={}, dst={}, hlen={}, size={}, checksum={}, ours={}", packet.len(), hdr.src, hdr.dst, hdr.hlen, hdr.len, hdr.sum, checksum);

                if checksum != hdr.sum {
                    Err(Error::InvalidPacket)?;
                }

                let packet = &packet[..len];
                let hdr_len = hdr.hlen as usize;
                if packet.len() < hdr_len {
                    Err(Error::DataUnderflow)?;
                }

                Ok(Some((hdr, &packet[hdr_len..])))
            } else {
                Ok(None)
            }
        }

        pub fn inject_checksum(packet: &mut [u8], checksum: u16) {
            let checksum = checksum.to_be_bytes();

            let offset = Self::CHECKSUM_WORD << 1;
            packet[offset] = checksum[0];
            packet[offset + 1] = checksum[1];
        }

        pub fn checksum(packet: &[u8]) -> u16 {
            let hlen = (packet[0] & 0x0f) as usize * 4;

            let sum = checksum_accumulate(&packet[..hlen], Self::CHECKSUM_WORD);

            checksum_finish(sum)
        }
    }

    #[derive(Clone, Debug)]
    pub struct UdpPacketHeader {
        pub src: u16, // Source port
        pub dst: u16, // Destination port
        pub len: u16, // UDP length
        pub sum: u16, // UDP checksum
    }

    impl UdpPacketHeader {
        pub const PROTO: u8 = 17;

        pub const SIZE: usize = 8;
        pub const CHECKSUM_WORD: usize = 3;

        pub fn new(src: u16, dst: u16) -> Self {
            Self {
                src,
                dst,
                len: 0,
                sum: 0,
            }
        }

        /// Parses the packet header from a byte slice
        pub fn decode(data: &[u8]) -> Result<Self, Error> {
            let mut bytes = BytesIn::new(data);

            Ok(Self {
                src: u16::from_be_bytes(bytes.arr()?),
                dst: u16::from_be_bytes(bytes.arr()?),
                len: u16::from_be_bytes(bytes.arr()?),
                sum: u16::from_be_bytes(bytes.arr()?),
            })
        }

        /// Encodes the packet header into the provided buf slice
        pub fn encode<'o>(&self, buf: &'o mut [u8]) -> Result<&'o [u8], Error> {
            let mut bytes = BytesOut::new(buf);

            bytes
                .push(&u16::to_be_bytes(self.src))?
                .push(&u16::to_be_bytes(self.dst))?
                .push(&u16::to_be_bytes(self.len))?
                .push(&u16::to_be_bytes(self.sum))?;

            let len = bytes.len();

            Ok(&buf[..len])
        }

        pub fn encode_with_payload<'o, F>(
            &mut self,
            buf: &'o mut [u8],
            ip_hdr: &Ipv4PacketHeader,
            encoder: F,
        ) -> Result<&'o [u8], Error>
        where
            F: FnOnce(&mut [u8]) -> Result<usize, Error>,
        {
            if buf.len() < Self::SIZE {
                Err(Error::BufferOverflow)?;
            }

            let (hdr_buf, payload_buf) = buf.split_at_mut(Self::SIZE);

            let payload_len = encoder(payload_buf)?;

            let len = Self::SIZE + payload_len;
            self.len = len as _;

            let hdr_len = self.encode(hdr_buf)?.len();
            assert_eq!(Self::SIZE, hdr_len);

            let packet = &mut buf[..len];

            let checksum = Self::checksum(packet, ip_hdr);
            self.sum = checksum;

            Self::inject_checksum(packet, checksum);

            Ok(packet)
        }

        pub fn decode_with_payload<'o>(
            packet: &'o [u8],
            ip_hdr: &Ipv4PacketHeader,
        ) -> Result<(Self, &'o [u8]), Error> {
            let hdr = Self::decode(packet)?;

            let len = hdr.len as usize;
            if packet.len() < len {
                Err(Error::DataUnderflow)?;
            }

            let checksum = Self::checksum(&packet[..len], ip_hdr);

            trace!(
                "UDP header decoded, src={}, dst={}, size={}, checksum={}, ours={}",
                hdr.src,
                hdr.dst,
                hdr.len,
                hdr.sum,
                checksum
            );

            if checksum != hdr.sum {
                Err(Error::InvalidPacket)?;
            }

            let packet = &packet[..len];

            let payload_data = &packet[Self::SIZE..];

            Ok((hdr, payload_data))
        }

        pub fn inject_checksum(packet: &mut [u8], checksum: u16) {
            let checksum = checksum.to_be_bytes();

            let offset = Self::CHECKSUM_WORD << 1;
            packet[offset] = checksum[0];
            packet[offset + 1] = checksum[1];
        }

        pub fn checksum(packet: &[u8], ip_hdr: &Ipv4PacketHeader) -> u16 {
            let mut buf = [0; 12];

            // Pseudo IP-header for UDP checksum calculation
            let len = BytesOut::new(&mut buf)
                .push(&u32::to_be_bytes(ip_hdr.src.into()))
                .unwrap()
                .push(&u32::to_be_bytes(ip_hdr.dst.into()))
                .unwrap()
                .byte(0)
                .unwrap()
                .byte(ip_hdr.p)
                .unwrap()
                .push(&u16::to_be_bytes(packet.len() as u16))
                .unwrap()
                .len();

            let sum = checksum_accumulate(&buf[..len], usize::MAX)
                + checksum_accumulate(packet, Self::CHECKSUM_WORD);

            checksum_finish(sum)
        }
    }

    pub fn checksum_accumulate(bytes: &[u8], checksum_word: usize) -> u32 {
        let mut bytes = BytesIn::new(bytes);

        let mut sum: u32 = 0;
        while !bytes.is_empty() {
            let skip = (bytes.offset() >> 1) == checksum_word;
            let arr = bytes
                .arr()
                .ok()
                .unwrap_or_else(|| [bytes.byte().unwrap(), 0]);

            let word = if skip { 0 } else { u16::from_be_bytes(arr) };

            sum += word as u32;
        }

        sum
    }

    pub fn checksum_finish(mut sum: u32) -> u16 {
        while sum >> 16 != 0 {
            sum = (sum >> 16) + (sum & 0xffff);
        }

        !sum as u16
    }
}
