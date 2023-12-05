use core::fmt::Debug;

use embedded_nal_async::{SocketAddr, SocketAddrV4, UdpStack, UnconnectedUdp};

use crate as dhcp;

pub mod client;
pub mod server;

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
