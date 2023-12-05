use core::fmt::Debug;

use embedded_nal_async::{SocketAddr, SocketAddrV4, UdpStack, UnconnectedUdp};

use crate as dhcp;

pub mod client;
pub mod server;

pub const DEFAULT_SERVER_PORT: u16 = 67;
pub const DEFAULT_CLIENT_PORT: u16 = 68;

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
