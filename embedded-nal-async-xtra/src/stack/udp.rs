use embedded_nal_async::{SocketAddr, UnconnectedUdp};

pub trait UnconnectedUdpWithMac: UnconnectedUdp {
    async fn send(
        &mut self,
        local: SocketAddr,
        remote: SocketAddr,
        remote_mac: Option<&[u8; 6]>,
        data: &[u8],
    ) -> Result<(), Self::Error>;

    async fn receive_into(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, SocketAddr, SocketAddr, [u8; 6]), Self::Error>;
}
