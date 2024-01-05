use core::marker::PhantomData;
use core::mem::MaybeUninit;

use embassy_futures::select::{select, Either};

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::zerocopy_channel::{Channel, Receiver, Sender};

pub use embedded_nal_async::{SocketAddr, UnconnectedUdp};

use super::MAX_TX_BUF_SIZE;

pub struct UdpSplitBuffer(MaybeUninit<[UdpPacket; 1]>);

impl UdpSplitBuffer {
    pub const fn new() -> Self {
        Self(MaybeUninit::uninit())
    }
}

pub struct UdpSplit<'a, M: RawMutex, S>(S, Channel<'a, M, UdpPacket>);

impl<'a, M: RawMutex, S> UdpSplit<'a, M, S>
where
    S: UnconnectedUdp,
{
    pub fn new(socket: S, buffer: &'a mut UdpSplitBuffer) -> Self {
        let channel = Channel::new(unsafe { buffer.0.assume_init_mut() });

        Self(socket, channel)
    }

    pub fn split(&mut self) -> (UdpSplitSend<'_, M, S>, UdpSplitReceive<'_, M, S>) {
        let (sender, receiver) = self.1.split();

        (
            UdpSplitSend(sender, PhantomData),
            UdpSplitReceive(&mut self.0, receiver),
        )
    }
}

struct UdpPacket {
    data: [u8; MAX_TX_BUF_SIZE],
    len: usize,
    local: SocketAddr,
    remote: SocketAddr,
}

pub struct UdpSplitSend<'a, M: RawMutex, S: UnconnectedUdp>(
    Sender<'a, M, UdpPacket>,
    PhantomData<fn() -> S::Error>,
);

impl<'a, M: RawMutex, S: UnconnectedUdp> UdpSplitSend<'a, M, S> {
    pub async fn send(
        &mut self,
        local: SocketAddr,
        remote: SocketAddr,
        data: &[u8],
    ) -> Result<(), S::Error> {
        let packet = self.0.send().await;

        packet.data[..data.len()].copy_from_slice(data);
        packet.len = data.len();
        packet.local = local;
        packet.remote = remote;

        self.0.send_done();

        Ok(())
    }
}

pub struct UdpSplitReceive<'a, M: RawMutex, S>(&'a mut S, Receiver<'a, M, UdpPacket>);

impl<'a, M: RawMutex, S: UnconnectedUdp> UdpSplitReceive<'a, M, S> {
    pub async fn receive_into(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, SocketAddr, SocketAddr), S::Error> {
        loop {
            let result = select(self.1.receive(), self.0.receive_into(buffer)).await;

            match result {
                Either::First(UdpPacket {
                    data,
                    len,
                    local,
                    remote,
                }) => {
                    self.0.send(*local, *remote, &data[..*len]).await?;
                    self.1.receive_done();
                }
                Either::Second(result) => break result,
            }
        }
    }
}
