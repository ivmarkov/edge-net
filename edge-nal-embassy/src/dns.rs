use core::net::IpAddr;

use edge_nal::AddrType;

use embassy_net::{
    dns::{DnsQueryType, Error},
    driver::Driver,
    Stack,
};
use embedded_io_async::ErrorKind;

use crate::to_net_addr;

/// A struct that implements the `Dns` trait from `edge-nal`
pub struct Dns<'a, D>
where
    D: Driver + 'static,
{
    stack: &'a Stack<D>,
}

impl<'a, D> Dns<'a, D>
where
    D: Driver + 'static,
{
    /// Create a new `Dns` instance for the provided Embassy networking stack
    ///
    /// NOTE: If using DHCP, make sure it has reconfigured the stack to ensure the DNS servers are updated
    pub fn new(stack: &'a Stack<D>) -> Self {
        Self { stack }
    }
}

impl<'a, D> edge_nal::Dns for Dns<'a, D>
where
    D: Driver + 'static,
{
    type Error = DnsError;

    async fn get_host_by_name(
        &self,
        host: &str,
        addr_type: AddrType,
    ) -> Result<IpAddr, Self::Error> {
        let qtype = match addr_type {
            AddrType::IPv6 => DnsQueryType::Aaaa,
            _ => DnsQueryType::A,
        };
        let addrs = self.stack.dns_query(host, qtype).await?;
        if let Some(first) = addrs.get(0) {
            Ok(to_net_addr(*first))
        } else {
            Err(Error::Failed.into())
        }
    }

    async fn get_host_by_address(
        &self,
        _addr: IpAddr,
        _result: &mut [u8],
    ) -> Result<usize, Self::Error> {
        todo!()
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct DnsError(Error);

impl From<Error> for DnsError {
    fn from(e: Error) -> Self {
        DnsError(e)
    }
}

// TODO
impl embedded_io_async::Error for DnsError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}
