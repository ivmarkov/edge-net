/// This code is a `no_std` and no-alloc modification of https://github.com/krolaw/dhcp4r
use core::str::Utf8Error;

use no_std_net::Ipv4Addr;

use num_enum::TryFromPrimitive;

#[derive(Debug)]
pub enum Error {
    DataUnderflow,
    InvalidUtf8Str(Utf8Error),
    InvalidMessageType,
    InvalidHlen,
    BufferOverflow,
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
    pub chaddr: [u8; 6],
    pub options: Options<'a>,
}

impl<'a> Packet<'a> {
    const COOKIE: [u8; 4] = [99, 130, 83, 99];

    const BOOT_REQUEST: u8 = 1; // From Client;
    const BOOT_REPLY: u8 = 2; // From Server;

    const END: u8 = 255;
    const PAD: u8 = 0;

    /// Parses Packet from byte array
    pub fn decode(data: &'a [u8]) -> Result<Self, Error> {
        let mut bytes = BytesIn::new(data);

        Ok(Self {
            reply: {
                let reply = bytes.byte()? == Self::BOOT_REPLY;
                let _xid = bytes.byte()?;
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
                bytes.slice(202)?;

                if bytes.arr()? != Self::COOKIE {
                    todo!()
                }

                Options(OptionsInner::decode(bytes.remaining())?)
            },
        })
    }

    /// Creates byte array DHCP packet
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
            .byte(0)?
            .push(&self.ciaddr.octets())?
            .push(&self.yiaddr.octets())?
            .push(&self.siaddr.octets())?
            .push(&self.giaddr.octets())?
            .push(&self.chaddr)?
            .push(&[0; 202])?
            .push(&Self::COOKIE)?;

        self.options.0.encode(&mut bytes)?;

        bytes.byte(Self::END)?;

        while bytes.len() < 272 {
            bytes.byte(Self::PAD)?;
        }

        let len = bytes.len();

        Ok(&buf[..len])
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Options<'a>(OptionsInner<'a>);

impl<'a> Options<'a> {
    pub const fn new(options: &'a [DhcpOption<'a>]) -> Self {
        Self(OptionsInner::DataSlice(options))
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DhcpOption<'a> {
    DhcpMessageType(MessageType),
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
    fn decode<'o>(bytes: &mut BytesIn<'o>) -> Result<Option<DhcpOption<'o>>, Error> {
        let code = bytes.byte()?;
        if code == Packet::END {
            Ok(None)
        } else {
            let len = bytes.byte()? as usize;
            let mut bytes = BytesIn::new(bytes.slice(len)?);

            let option = match code {
                DHCP_MESSAGE_TYPE => DhcpOption::DhcpMessageType(
                    TryFromPrimitive::try_from_primitive(bytes.byte()?)
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
            out.push(data)?;
            Ok(())
        })
    }

    fn code(&self) -> u8 {
        match self {
            Self::DhcpMessageType(_) => DHCP_MESSAGE_TYPE,
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
            Self::DhcpMessageType(mtype) => f(&[*mtype as _]),
            Self::ServerIdentifier(addr) => f(&addr.octets()),
            Self::ParameterRequestList(prl) => f(*prl),
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ipv4Addrs<'a>(Ipv4AddrsInner<'a>);

impl<'a> Ipv4Addrs<'a> {
    pub const fn new(addrs: &'a [Ipv4Addr]) -> Self {
        Self(Ipv4AddrsInner::DataSlice(addrs))
    }

    pub fn iter(&self) -> impl Iterator<Item = Ipv4Addr> + 'a {
        self.0.iter()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
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

    pub fn remaining_arr<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let arr = self.arr::<N>()?;

        if self.is_empty() {
            Ok(arr)
        } else {
            Err(Error::BufferOverflow) // TODO
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

///////////////////////////////

// // DHCP Options;
pub const SUBNET_MASK: u8 = 1;
// pub const TIME_OFFSET: u8 = 2;
pub const ROUTER: u8 = 3;
// pub const TIME_SERVER: u8 = 4;
// pub const NAME_SERVER: u8 = 5;
pub const DOMAIN_NAME_SERVER: u8 = 6;
// pub const LOG_SERVER: u8 = 7;
// pub const COOKIE_SERVER: u8 = 8;
// pub const LPR_SERVER: u8 = 9;
// pub const IMPRESS_SERVER: u8 = 10;
// pub const RESOURCE_LOCATION_SERVER: u8 = 11;
pub const HOST_NAME: u8 = 12;
// pub const BOOT_FILE_SIZE: u8 = 13;
// pub const MERIT_DUMP_FILE: u8 = 14;
// pub const DOMAIN_NAME: u8 = 15;
// pub const SWAP_SERVER: u8 = 16;
// pub const ROOT_PATH: u8 = 17;
// pub const EXTENSIONS_PATH: u8 = 18;

// // IP LAYER PARAMETERS PER HOST;
// pub const IP_FORWARDING_ENABLE_DISABLE: u8 = 19;
// pub const NON_LOCAL_SOURCE_ROUTING_ENABLE_DISABLE: u8 = 20;
// pub const POLICY_FILTER: u8 = 21;
// pub const MAXIMUM_DATAGRAM_REASSEMBLY_SIZE: u8 = 22;
// pub const DEFAULT_IP_TIME_TO_LIVE: u8 = 23;
// pub const PATH_MTU_AGING_TIMEOUT: u8 = 24;
// pub const PATH_MTU_PLATEAU_TABLE: u8 = 25;

// // IP LAYER PARAMETERS PER INTERFACE;
// pub const INTERFACE_MTU: u8 = 26;
// pub const ALL_SUBNETS_ARE_LOCAL: u8 = 27;
// pub const BROADCAST_ADDRESS: u8 = 28;
// pub const PERFORM_MASK_DISCOVERY: u8 = 29;
// pub const MASK_SUPPLIER: u8 = 30;
// pub const PERFORM_ROUTER_DISCOVERY: u8 = 31;
// pub const ROUTER_SOLICITATION_ADDRESS: u8 = 32;
// pub const STATIC_ROUTE: u8 = 33;

// // LINK LAYER PARAMETERS PER INTERFACE;
// pub const TRAILER_ENCAPSULATION: u8 = 34;
// pub const ARP_CACHE_TIMEOUT: u8 = 35;
// pub const ETHERNET_ENCAPSULATION: u8 = 36;

// // TCP PARAMETERS;
// pub const TCP_DEFAULT_TTL: u8 = 37;
// pub const TCP_KEEPALIVE_INTERVAL: u8 = 38;
// pub const TCP_KEEPALIVE_GARBAGE: u8 = 39;

// // APPLICATION AND SERVICE PARAMETERS;
// pub const NETWORK_INFORMATION_SERVICE_DOMAIN: u8 = 40;
// pub const NETWORK_INFORMATION_SERVERS: u8 = 41;
// pub const NETWORK_TIME_PROTOCOL_SERVERS: u8 = 42;
// pub const VENDOR_SPECIFIC_INFORMATION: u8 = 43;
// pub const NETBIOS_OVER_TCPIP_NAME_SERVER: u8 = 44;
// pub const NETBIOS_OVER_TCPIP_DATAGRAM_DISTRIBUTION_SERVER: u8 = 45;
// pub const NETBIOS_OVER_TCPIP_NODE_TYPE: u8 = 46;
// pub const NETBIOS_OVER_TCPIP_SCOPE: u8 = 47;
// pub const XWINDOW_SYSTEM_FONT_SERVER: u8 = 48;
// pub const XWINDOW_SYSTEM_DISPLAY_MANAGER: u8 = 49;
// pub const NETWORK_INFORMATION_SERVICEPLUS_DOMAIN: u8 = 64;
// pub const NETWORK_INFORMATION_SERVICEPLUS_SERVERS: u8 = 65;
// pub const MOBILE_IP_HOME_AGENT: u8 = 68;
// pub const SIMPLE_MAIL_TRANSPORT_PROTOCOL: u8 = 69;
// pub const POST_OFFICE_PROTOCOL_SERVER: u8 = 70;
// pub const NETWORK_NEWS_TRANSPORT_PROTOCOL: u8 = 71;
// pub const DEFAULT_WORLD_WIDE_WEB_SERVER: u8 = 72;
// pub const DEFAULT_FINGER_SERVER: u8 = 73;
// pub const DEFAULT_INTERNET_RELAY_CHAT_SERVER: u8 = 74;
// pub const STREETTALK_SERVER: u8 = 75;
// pub const STREETTALK_DIRECTORY_ASSISTANCE: u8 = 76;

// pub const RELAY_AGENT_INFORMATION: u8 = 82;

// // DHCP EXTENSIONS
pub const REQUESTED_IP_ADDRESS: u8 = 50;
pub const IP_ADDRESS_LEASE_TIME: u8 = 51;
// pub const OVERLOAD: u8 = 52;
pub const DHCP_MESSAGE_TYPE: u8 = 53;
pub const SERVER_IDENTIFIER: u8 = 54;
pub const PARAMETER_REQUEST_LIST: u8 = 55;
pub const MESSAGE: u8 = 56;
// pub const MAXIMUM_DHCP_MESSAGE_SIZE: u8 = 57;
// pub const RENEWAL_TIME_VALUE: u8 = 58;
// pub const REBINDING_TIME_VALUE: u8 = 59;
// pub const VENDOR_CLASS_IDENTIFIER: u8 = 60;
// pub const CLIENT_IDENTIFIER: u8 = 61;

// pub const TFTP_SERVER_NAME: u8 = 66;
// pub const BOOTFILE_NAME: u8 = 67;

// pub const USER_CLASS: u8 = 77;

// pub const CLIENT_ARCHITECTURE: u8 = 93;

// pub const TZ_POSIX_STRING: u8 = 100;
// pub const TZ_DATABASE_STRING: u8 = 101;

// pub const CLASSLESS_ROUTE_FORMAT: u8 = 121;

// /// Returns title of DHCP Option code, if known.
// pub fn title(code: u8) -> Option<&'static str> {
//     Some(match code {
//         SUBNET_MASK => "Subnet Mask",

//         TIME_OFFSET => "Time Offset",
//         ROUTER => "Router",
//         TIME_SERVER => "Time Server",
//         NAME_SERVER => "Name Server",
//         DOMAIN_NAME_SERVER => "Domain Name Server",
//         LOG_SERVER => "Log Server",
//         COOKIE_SERVER => "Cookie Server",
//         LPR_SERVER => "LPR Server",
//         IMPRESS_SERVER => "Impress Server",
//         RESOURCE_LOCATION_SERVER => "Resource Location Server",
//         HOST_NAME => "Host Name",
//         BOOT_FILE_SIZE => "Boot File Size",
//         MERIT_DUMP_FILE => "Merit Dump File",
//         DOMAIN_NAME => "Domain Name",
//         SWAP_SERVER => "Swap Server",
//         ROOT_PATH => "Root Path",
//         EXTENSIONS_PATH => "Extensions Path",

//         // IP LAYER PARAMETERS PER HOST",
//         IP_FORWARDING_ENABLE_DISABLE => "IP Forwarding Enable/Disable",
//         NON_LOCAL_SOURCE_ROUTING_ENABLE_DISABLE => "Non-Local Source Routing Enable/Disable",
//         POLICY_FILTER => "Policy Filter",
//         MAXIMUM_DATAGRAM_REASSEMBLY_SIZE => "Maximum Datagram Reassembly Size",
//         DEFAULT_IP_TIME_TO_LIVE => "Default IP Time-to-live",
//         PATH_MTU_AGING_TIMEOUT => "Path MTU Aging Timeout",
//         PATH_MTU_PLATEAU_TABLE => "Path MTU Plateau Table",

//         // IP LAYER PARAMETERS PER INTERFACE",
//         INTERFACE_MTU => "Interface MTU",
//         ALL_SUBNETS_ARE_LOCAL => "All Subnets are Local",
//         BROADCAST_ADDRESS => "Broadcast Address",
//         PERFORM_MASK_DISCOVERY => "Perform Mask Discovery",
//         MASK_SUPPLIER => "Mask Supplier",
//         PERFORM_ROUTER_DISCOVERY => "Perform Router Discovery",
//         ROUTER_SOLICITATION_ADDRESS => "Router Solicitation Address",
//         STATIC_ROUTE => "Static Route",

//         // LINK LAYER PARAMETERS PER INTERFACE",
//         TRAILER_ENCAPSULATION => "Trailer Encapsulation",
//         ARP_CACHE_TIMEOUT => "ARP Cache Timeout",
//         ETHERNET_ENCAPSULATION => "Ethernet Encapsulation",

//         // TCP PARAMETERS",
//         TCP_DEFAULT_TTL => "TCP Default TTL",
//         TCP_KEEPALIVE_INTERVAL => "TCP Keepalive Interval",
//         TCP_KEEPALIVE_GARBAGE => "TCP Keepalive Garbage",

//         // APPLICATION AND SERVICE PARAMETERS",
//         NETWORK_INFORMATION_SERVICE_DOMAIN => "Network Information Service Domain",
//         NETWORK_INFORMATION_SERVERS => "Network Information Servers",
//         NETWORK_TIME_PROTOCOL_SERVERS => "Network Time Protocol Servers",
//         VENDOR_SPECIFIC_INFORMATION => "Vendor Specific Information",
//         NETBIOS_OVER_TCPIP_NAME_SERVER => "NetBIOS over TCP/IP Name Server",
//         NETBIOS_OVER_TCPIP_DATAGRAM_DISTRIBUTION_SERVER => {
//             "NetBIOS over TCP/IP Datagram Distribution Server"
//         }
//         NETBIOS_OVER_TCPIP_NODE_TYPE => "NetBIOS over TCP/IP Node Type",
//         NETBIOS_OVER_TCPIP_SCOPE => "NetBIOS over TCP/IP Scope",
//         XWINDOW_SYSTEM_FONT_SERVER => "X Window System Font Server",
//         XWINDOW_SYSTEM_DISPLAY_MANAGER => "X Window System Display Manager",
//         NETWORK_INFORMATION_SERVICEPLUS_DOMAIN => "Network Information Service+ Domain",
//         NETWORK_INFORMATION_SERVICEPLUS_SERVERS => "Network Information Service+ Servers",
//         MOBILE_IP_HOME_AGENT => "Mobile IP Home Agent",
//         SIMPLE_MAIL_TRANSPORT_PROTOCOL => "Simple Mail Transport Protocol (SMTP) Server",
//         POST_OFFICE_PROTOCOL_SERVER => "Post Office Protocol (POP3) Server",
//         NETWORK_NEWS_TRANSPORT_PROTOCOL => "Network News Transport Protocol (NNTP) Server",
//         DEFAULT_WORLD_WIDE_WEB_SERVER => "Default World Wide Web (WWW) Server",
//         DEFAULT_FINGER_SERVER => "Default Finger Server",
//         DEFAULT_INTERNET_RELAY_CHAT_SERVER => "Default Internet Relay Chat (IRC) Server",
//         STREETTALK_SERVER => "StreetTalk Server",
//         STREETTALK_DIRECTORY_ASSISTANCE => "StreetTalk Directory Assistance (STDA) Server",

//         RELAY_AGENT_INFORMATION => "Relay Agent Information",

//         // DHCP EXTENSIONS
//         REQUESTED_IP_ADDRESS => "Requested IP Address",
//         IP_ADDRESS_LEASE_TIME => "IP Address Lease Time",
//         OVERLOAD => "Overload",
//         DHCP_MESSAGE_TYPE => "DHCP Message Type",
//         SERVER_IDENTIFIER => "Server Identifier",
//         PARAMETER_REQUEST_LIST => "Parameter Request List",
//         MESSAGE => "Message",
//         MAXIMUM_DHCP_MESSAGE_SIZE => "Maximum DHCP Message Size",
//         RENEWAL_TIME_VALUE => "Renewal (T1) Time Value",
//         REBINDING_TIME_VALUE => "Rebinding (T2) Time Value",
//         VENDOR_CLASS_IDENTIFIER => "Vendor class identifier",
//         CLIENT_IDENTIFIER => "Client-identifier",

//         // Find below
//         TFTP_SERVER_NAME => "TFTP server name",
//         BOOTFILE_NAME => "Bootfile name",

//         USER_CLASS => "User Class",

//         CLIENT_ARCHITECTURE => "Client Architecture",

//         TZ_POSIX_STRING => "TZ-POSIX String",
//         TZ_DATABASE_STRING => "TZ-Database String",
//         CLASSLESS_ROUTE_FORMAT => "Classless Route Format",

//         _ => return None,
//     })
// }
