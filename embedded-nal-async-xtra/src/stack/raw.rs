pub trait RawSocket {
    type Error: embedded_io_async::Error;

    async fn send(&mut self, mac: Option<&[u8; 6]>, data: &[u8]) -> Result<(), Self::Error>;
    async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<(usize, [u8; 6]), Self::Error>;
}

impl<T> RawSocket for &mut T
where
    T: RawSocket,
{
    type Error = T::Error;

    async fn send(&mut self, mac: Option<&[u8; 6]>, data: &[u8]) -> Result<(), Self::Error> {
        (**self).send(mac, data).await
    }

    async fn receive_into(&mut self, buffer: &mut [u8]) -> Result<(usize, [u8; 6]), Self::Error> {
        (**self).receive_into(buffer).await
    }
}

pub trait RawStack {
    type Error: embedded_io_async::Error;

    type Socket: RawSocket<Error = Self::Error>;

    async fn bind(&self, interface: u32) -> Result<Self::Socket, Self::Error>;
}

impl<T> RawStack for &T
where
    T: RawStack,
{
    type Error = T::Error;

    type Socket = T::Socket;

    async fn bind(&self, interface: u32) -> Result<Self::Socket, Self::Error> {
        (*self).bind(interface).await
    }
}

impl<T> RawStack for &mut T
where
    T: RawStack,
{
    type Error = T::Error;

    type Socket = T::Socket;

    async fn bind(&self, interface: u32) -> Result<Self::Socket, Self::Error> {
        (**self).bind(interface).await
    }
}
