use embedded_nal_async::IpAddr;

pub trait Multicast {
    type Error: embedded_io_async::Error;

    async fn join(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error>;
    async fn leave(&mut self, multicast_addr: IpAddr) -> Result<(), Self::Error>;
}
