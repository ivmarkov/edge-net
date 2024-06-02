use core::net::{Ipv4Addr, Ipv6Addr};

use embedded_io_async::ErrorType;

pub trait MulticastV4: ErrorType {
    async fn join_v4(
        &mut self,
        multicast_addr: Ipv4Addr,
        interface: Ipv4Addr,
    ) -> Result<(), Self::Error>;
    async fn leave_v4(
        &mut self,
        multicast_addr: Ipv4Addr,
        interface: Ipv4Addr,
    ) -> Result<(), Self::Error>;
}

impl<T> MulticastV4 for &mut T
where
    T: MulticastV4,
{
    async fn join_v4(
        &mut self,
        multicast_addr: Ipv4Addr,
        interface: Ipv4Addr,
    ) -> Result<(), Self::Error> {
        (**self).join_v4(multicast_addr, interface).await
    }

    async fn leave_v4(
        &mut self,
        multicast_addr: Ipv4Addr,
        interface: Ipv4Addr,
    ) -> Result<(), Self::Error> {
        (**self).leave_v4(multicast_addr, interface).await
    }
}

pub trait MulticastV6: ErrorType {
    async fn join_v6(
        &mut self,
        multicast_addr: Ipv6Addr,
        interface: u32,
    ) -> Result<(), Self::Error>;
    async fn leave_v6(
        &mut self,
        multicast_addr: Ipv6Addr,
        interface: u32,
    ) -> Result<(), Self::Error>;
}

impl<T> MulticastV6 for &mut T
where
    T: MulticastV6,
{
    async fn join_v6(
        &mut self,
        multicast_addr: Ipv6Addr,
        interface: u32,
    ) -> Result<(), Self::Error> {
        (**self).join_v6(multicast_addr, interface).await
    }

    async fn leave_v6(
        &mut self,
        multicast_addr: Ipv6Addr,
        interface: u32,
    ) -> Result<(), Self::Error> {
        (**self).leave_v6(multicast_addr, interface).await
    }
}
