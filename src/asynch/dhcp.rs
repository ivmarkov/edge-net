use core::fmt::Debug;

use embedded_nal_async::{SocketAddr, SocketAddrV4, UdpStack, UnconnectedUdp};

use crate::dhcp;

use super::tcp::{RawSocket, RawStack};

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

pub trait SocketFactory {
    type Error: Debug;

    type Socket: Socket<Error = Self::Error>;

    fn raw_ports(&self) -> (Option<u16>, Option<u16>);

    async fn connect(&self) -> Result<Self::Socket, Self::Error>;
}

impl<T> SocketFactory for &T
where
    T: SocketFactory,
{
    type Error = T::Error;

    type Socket = T::Socket;

    fn raw_ports(&self) -> (Option<u16>, Option<u16>) {
        (*self).raw_ports()
    }

    async fn connect(&self) -> Result<Self::Socket, Self::Error> {
        (*self).connect().await
    }
}

impl<T> SocketFactory for &mut T
where
    T: SocketFactory,
{
    type Error = T::Error;

    type Socket = T::Socket;

    fn raw_ports(&self) -> (Option<u16>, Option<u16>) {
        (**self).raw_ports()
    }

    async fn connect(&self) -> Result<Self::Socket, Self::Error> {
        (**self).connect().await
    }
}

pub trait Socket {
    type Error: Debug;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
}

// impl<T> Socket for &mut T
// where
//     T: Socket,
// {
//     type Error = T::Error;

//     async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
//         (**self).send(data).await
//     }

//     async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
//         (**self).recv(buf).await
//     }
// }

pub struct RawSocketFactory<R> {
    stack: R,
    interface: Option<u32>,
    client_port: Option<u16>,
    server_port: Option<u16>,
}

impl<R> RawSocketFactory<R>
where
    R: RawStack,
{
    pub const fn new(
        stack: R,
        interface: Option<u32>,
        client_port: Option<u16>,
        server_port: Option<u16>,
    ) -> Self {
        if client_port.is_none() && server_port.is_none() {
            panic!("Either the client, or the sererver port, or both should be specified");
        }

        Self {
            stack,
            interface,
            client_port,
            server_port,
        }
    }
}

impl<R> SocketFactory for RawSocketFactory<R>
where
    R: RawStack,
{
    type Error = R::Error;

    type Socket = R::Socket;

    fn raw_ports(&self) -> (Option<u16>, Option<u16>) {
        (self.client_port, self.server_port)
    }

    async fn connect(&self) -> Result<Self::Socket, Self::Error> {
        self.stack.connect(self.interface).await
    }
}

impl<S> Socket for S
where
    S: RawSocket,
{
    type Error = S::Error;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        RawSocket::send(self, data).await
    }

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        RawSocket::receive_into(self, buf).await
    }
}

/// NOTE: This socket factory can only be used for the DHCP server
/// DHCP client *has* to run via raw sockets
pub struct UdpServerSocketFactory<U> {
    stack: U,
    local: SocketAddrV4,
}

impl<U> UdpServerSocketFactory<U>
where
    U: UdpStack,
{
    pub const fn new(stack: U, local: SocketAddrV4) -> Self {
        Self { stack, local }
    }
}

impl<U> SocketFactory for UdpServerSocketFactory<U>
where
    U: UdpStack,
{
    type Error = U::Error;

    type Socket = UdpServerSocket<U::UniquelyBound>;

    fn raw_ports(&self) -> (Option<u16>, Option<u16>) {
        (None, None)
    }

    async fn connect(&self) -> Result<Self::Socket, Self::Error> {
        let (local, socket) = self.stack.bind_single(SocketAddr::V4(self.local)).await?;

        Ok(UdpServerSocket {
            socket,
            local,
            remote: None,
        })
    }
}

pub struct UdpServerSocket<S> {
    socket: S,
    local: SocketAddr,
    remote: Option<SocketAddr>,
}

impl<S> Socket for UdpServerSocket<S>
where
    S: UnconnectedUdp,
{
    type Error = S::Error;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let remote = self
            .remote
            .expect("Sending is possible only after receiving a datagram");

        self.socket.send(self.local, remote, data).await
    }

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let (len, local, remote) = self.socket.receive_into(buf).await?;

        self.local = local;
        self.remote = Some(remote);

        Ok(len)
    }
}

pub mod client {
    use core::fmt::Debug;

    use embassy_futures::select::{select, Either};
    use embassy_time::{Duration, Instant, Timer};

    use embedded_nal_async::Ipv4Addr;

    use log::{info, warn};

    use rand_core::RngCore;

    use self::dhcp::MessageType;

    pub use super::*;

    pub use crate::dhcp::Settings;

    #[derive(Clone, Debug)]
    pub struct Configuration {
        pub mac: [u8; 6],
        pub timeout: Duration,
    }

    impl Configuration {
        pub const fn new(mac: [u8; 6]) -> Self {
            Self {
                mac,
                timeout: Duration::from_secs(10),
            }
        }
    }

    /// A simple asynchronous DHCP client.
    ///
    /// The client takes a socket factory (either operating on raw sockets or UDP datagrams) and
    /// then takes care of the all the negotiations with the DHCP server, as in discovering servers,
    /// negotiating initial IP, and then keeping the lease of that IP up to date.
    ///
    /// Note that it is unlikely that a non-raw socket factory would actually even work, due to the peculiarities of the
    /// DHCP protocol, where a lot of UDP packets are send (and often broasdcasted_) by the client before the client actually has an assigned IP.
    pub struct Client<T> {
        rng: T,
        mac: [u8; 6],
        timeout: Duration,
        settings: Option<(Settings, Instant)>,
    }

    impl<T> Client<T>
    where
        T: RngCore,
    {
        pub fn new(rng: T, conf: &Configuration) -> Self {
            info!("Creating DHCP client with configuration {conf:?}");

            Self {
                rng,
                mac: conf.mac,
                timeout: conf.timeout,
                settings: None,
            }
        }

        /// Runs the DHCP client with the supplied socket factory, and takes care of
        /// all aspects of negotiating an IP with the first DHCP server that replies to the discovery requests.
        ///
        /// From the POV of the user, this method will return only in two cases, which are exactly the cases where the user is expected to take an action:
        /// - When an initial/new IP lease was negotiated; in that case, `Some(Settings)` is returned, and the user should assign the returned IP settings
        ///   to the network interface using platform-specific means
        /// - When the IP lease was lost; in that case, `None` is returned, and the user should de-assign all IP settings from the network interface using
        ///   platform-specific means
        ///
        /// In both cases, user is expected to call `run` again, so that the IP lease is kept up to date / a new lease is re-negotiated
        ///
        /// Note that dropping this future is also safe in that it won't remove the current lease, so the user can renew
        /// the operation of the server by just calling `run` later on. Of course, if the future is not polled, the client
        /// would be unable - during that time - to check for lease timeout and the lease might not be renewed on time.
        ///
        /// But in any case, if the lease is expired or the DHCP server does not acknowledge the lease renewal, the client will
        /// automatically restart the DHCP servers' discovery from the very beginning.
        pub async fn run<F: SocketFactory>(
            &mut self,
            mut f: F,
            buf: &mut [u8],
        ) -> Result<Option<Settings>, Error<F::Error>> {
            loop {
                if let Some((settings, acquired)) = self.settings.as_ref() {
                    // Keep the lease
                    let now = Instant::now();

                    if now - *acquired
                        >= Duration::from_secs(settings.lease_time_secs.unwrap_or(7200) as u64 / 3)
                    {
                        info!("Renewing DHCP lease...");

                        if let Some(settings) = self
                            .request(&mut f, buf, settings.server_ip.unwrap(), settings.ip)
                            .await?
                        {
                            self.settings = Some((settings, Instant::now()));
                        } else {
                            // Lease was not renewed; let the user know
                            self.settings = None;

                            return Ok(None);
                        }
                    } else {
                        Timer::after(Duration::from_secs(60)).await;
                    }
                } else {
                    // Look for offers
                    let offer = self.discover(&mut f, buf).await?;

                    if let Some(settings) = self
                        .request(&mut f, buf, offer.server_ip.unwrap(), offer.ip)
                        .await?
                    {
                        // IP acquired; let the user know
                        self.settings = Some((settings.clone(), Instant::now()));

                        return Ok(Some(settings));
                    }
                }
            }
        }

        /// This method allows the user to inform the DHCP server that the currently leased IP (if any) is no longer used
        /// by the client.
        ///
        /// Useful when the program runnuing the DHCP client is about to exit.
        pub async fn release<F: SocketFactory>(
            &mut self,
            f: F,
            buf: &mut [u8],
        ) -> Result<(), Error<F::Error>> {
            if let Some((settings, _)) = self.settings.as_ref().cloned() {
                let mut socket = f.connect().await.map_err(Error::Io)?;

                let packet = self.client(&f).encode_release(
                    buf,
                    0,
                    settings.server_ip.unwrap(),
                    settings.ip,
                )?;

                socket.send(packet).await.map_err(Error::Io)?;
            }

            self.settings = None;

            Ok(())
        }

        async fn discover<F: SocketFactory>(
            &mut self,
            f: &mut F,
            buf: &mut [u8],
        ) -> Result<Settings, Error<F::Error>> {
            info!("Discovering DHCP servers...");

            let timeout = self.timeout;
            let mut client = self.client(&f);

            let start = Instant::now();

            loop {
                let mut socket = f.connect().await.map_err(Error::Io)?;

                let (packet, xid) =
                    client.encode_discover(buf, (Instant::now() - start).as_secs() as _, None)?;

                socket.send(packet).await.map_err(Error::Io)?;

                let offer_start = Instant::now();

                while Instant::now() - offer_start < timeout {
                    let timer = Timer::after(Duration::from_secs(3));

                    if let Either::First(result) = select(socket.recv(buf), timer).await {
                        let len = result.map_err(Error::Io)?;
                        let packet = &buf[..len];

                        if let Some(reply) =
                            client.decode_bootp_reply(packet, xid, Some(&[MessageType::Offer]))?
                        {
                            let settings = reply.settings().unwrap().1;

                            info!(
                                "IP {} offered by DHCP server {}",
                                settings.ip,
                                settings.server_ip.unwrap()
                            );
                            return Ok(settings);
                        }
                    }
                }

                drop(socket);

                info!("No DHCP offers received, sleeping for a while...");

                Timer::after(Duration::from_secs(3)).await;
            }
        }

        async fn request<F: SocketFactory>(
            &mut self,
            f: &mut F,
            buf: &mut [u8],
            server_ip: Ipv4Addr,
            ip: Ipv4Addr,
        ) -> Result<Option<Settings>, Error<F::Error>> {
            let timeout = self.timeout;
            let mut client = self.client(&f);

            for _ in 0..3 {
                info!("Requesting IP {ip} from DHCP server {server_ip}");

                let mut socket = f.connect().await.map_err(Error::Io)?;

                let start = Instant::now();

                let (packet, xid) = client.encode_request(
                    buf,
                    (Instant::now() - start).as_secs() as _,
                    server_ip,
                    ip,
                )?;

                socket.send(packet).await.map_err(Error::Io)?;

                let request_start = Instant::now();

                while Instant::now() - request_start < timeout {
                    let timer = Timer::after(Duration::from_secs(10));

                    if let Either::First(result) = select(socket.recv(buf), timer).await {
                        let len = result.map_err(Error::Io)?;
                        let packet = &buf[..len];

                        if let Some(reply) = client.decode_bootp_reply(
                            packet,
                            xid,
                            Some(&[MessageType::Ack, MessageType::Nak]),
                        )? {
                            let (mt, settings) = reply.settings().unwrap();

                            let settings = if matches!(mt, MessageType::Ack) {
                                info!("IP {} leased successfully", settings.ip);
                                Some(settings)
                            } else {
                                info!("IP {} not acknowledged", settings.ip);
                                None
                            };

                            return Ok(settings);
                        }
                    }
                }

                drop(socket);
            }

            warn!("IP request was not replied");

            Ok(None)
        }

        fn client<F: SocketFactory>(&mut self, f: F) -> dhcp::client::Client<&mut T> {
            dhcp::client::Client {
                rng: &mut self.rng,
                mac: self.mac,
                rp_udp_client_port: f.raw_ports().0,
                rp_udp_server_port: f.raw_ports().1,
            }
        }
    }
}

pub mod server {
    use core::fmt::Debug;

    use embassy_time::Duration;

    use embedded_nal_async::Ipv4Addr;

    pub use super::*;

    #[derive(Clone, Debug)]
    pub struct Configuration {
        pub ip: Ipv4Addr,
        pub gateway: Option<Ipv4Addr>,
        pub subnet: Option<Ipv4Addr>,
        pub dns1: Option<Ipv4Addr>,
        pub dns2: Option<Ipv4Addr>,
        pub range_start: Ipv4Addr,
        pub range_end: Ipv4Addr,
        pub lease_duration_secs: u32,
    }

    /// A simple asynchronous DHCP server.
    ///
    /// The client takes a socket factory (either operating on raw sockets or UDP datagrams) and
    /// then processes all incoming BOOTP requests, by updating its internal simple database of leases, and issuing replies.
    pub struct Server<const N: usize> {
        pub server: dhcp::server::Server<N>,
    }

    impl<const N: usize> Server<N> {
        pub fn new(conf: &Configuration) -> Self {
            Self {
                server: dhcp::server::Server {
                    ip: conf.ip,
                    gateways: conf.gateway.iter().cloned().collect(),
                    subnet: conf.subnet,
                    dns: conf.dns1.iter().chain(conf.dns2.iter()).cloned().collect(),
                    range_start: conf.range_start,
                    range_end: conf.range_end,
                    lease_duration: Duration::from_secs(conf.lease_duration_secs as _),
                    leases: heapless::LinearMap::new(),
                },
            }
        }

        /// Runs the DHCP server wth the supplied socket factory, processing incoming DHCP requests.
        ///
        /// Note that dropping this future is safe in that it won't remove the internal leases' database,
        /// so users are free to drop the future in case they would like to take a snapshot of the leases or inspect them otherwise.
        pub async fn run<F: SocketFactory>(
            &mut self,
            f: F,
            buf: &mut [u8],
        ) -> Result<(), Error<F::Error>> {
            let mut socket = f.connect().await.map_err(Error::Io)?;

            loop {
                let len = socket.recv(buf).await.map_err(Error::Io)?;

                if let Some(reply) = self
                    .server
                    .handle_bootp_request(f.raw_ports().1, buf, len)?
                {
                    socket.send(reply).await.map_err(Error::Io)?;
                }
            }
        }
    }
}
