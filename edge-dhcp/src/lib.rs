#![no_std]

/// This code is a `no_std` and no-alloc modification of https://github.com/krolaw/dhcp4r
use core::str::Utf8Error;

use no_std_net::Ipv4Addr;

use num_enum::TryFromPrimitive;

use edge_raw::bytes::{self, BytesIn, BytesOut};

pub mod client;
pub mod server;

#[cfg(feature = "nightly")]
pub mod io;

#[derive(Debug)]
pub enum Error {
    DataUnderflow,
    BufferOverflow,
    InvalidPacket,
    InvalidUtf8Str(Utf8Error),
    InvalidMessageType,
    MissingCookie,
    InvalidHlen,
}

impl From<bytes::Error> for Error {
    fn from(value: bytes::Error) -> Self {
        match value {
            bytes::Error::BufferOverflow => Self::BufferOverflow,
            bytes::Error::DataUnderflow => Self::DataUnderflow,
            bytes::Error::InvalidFormat => Self::InvalidPacket,
        }
    }
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
