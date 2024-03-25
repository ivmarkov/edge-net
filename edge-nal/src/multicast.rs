use core::net::IpAddr;

use embedded_io_async::ErrorType;

pub trait Multicast: ErrorType {
    async fn join(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error>;
    async fn leave(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error>;
}

impl<T> Multicast for &mut T
where
    T: Multicast,
{
    async fn join(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error> {
        (**self).join(multicast_addr).await
    }

    async fn leave(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error> {
        (**self).leave(multicast_addr).await
    }
}
