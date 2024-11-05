use core::fmt::{self, Debug};
use core::net::{SocketAddr, SocketAddrV4};

use crate as dhcp;

pub mod client;
pub mod server;

pub const DEFAULT_SERVER_PORT: u16 = 67;
pub const DEFAULT_CLIENT_PORT: u16 = 68;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error<E> {
    Io(E),
    Format(dhcp::Error),
}

impl<E> From<dhcp::Error> for Error<E> {
    fn from(value: dhcp::Error) -> Self {
        Self::Format(value)
    }
}

impl<E> fmt::Display for Error<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "IO error: {err}"),
            Self::Format(err) => write!(f, "Format error: {err}"),
        }
    }
}

#[cfg(feature = "std")]
impl<E> std::error::Error for Error<E> where E: std::error::Error {}
