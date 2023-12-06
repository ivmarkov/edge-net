use core::fmt::Debug;

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer};

use embedded_nal_async::{ConnectedUdp, Ipv4Addr};

use log::{info, warn};

use rand_core::RngCore;

pub use super::*;

pub use crate::Settings;
use crate::{Options, Packet};

#[derive(Clone, Debug)]
pub struct Configuration {
    pub socket: SocketAddrV4,
    pub server_port: u16,
    pub mac: [u8; 6],
    pub timeout: Duration,
}

impl Configuration {
    pub const fn new(mac: [u8; 6]) -> Self {
        Self {
            socket: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_CLIENT_PORT),
            server_port: DEFAULT_SERVER_PORT,
            mac,
            timeout: Duration::from_secs(10),
        }
    }
}

/// A simple asynchronous DHCP client.
///
/// The client takes a UDP socket stack and then takes care of the all the negotiations with the DHCP server,
/// as in discovering servers, negotiating initial IP, and then keeping the lease of that IP up to date.
///
/// Note that the `UdpStack` implementation that the client takes need to be a bit special, because the DHCP client operates
/// before it has an IP address assigned:
/// - It needs to be able to send broadcast packets (to IP 255.255.255.255)
/// - It needs to be able to receive broadcast packets
///
/// Usually, this is achieved by utilizing raw sockets. One such socket stack is `Udp2RawStack` in the `edge-raw` crate.
pub struct Client<'a, T, F> {
    stack: F,
    buf: &'a mut [u8],
    client: dhcp::client::Client<T>,
    socket: SocketAddrV4,
    server_port: u16,
    timeout: Duration,
    pub settings: Option<(Settings, Instant)>,
}

impl<'a, T, F> Client<'a, T, F>
where
    T: RngCore,
    F: UdpStack,
{
    pub fn new(stack: F, buf: &'a mut [u8], rng: T, conf: &Configuration) -> Self {
        info!("Creating DHCP client with configuration {conf:?}");

        Self {
            stack,
            buf,
            client: dhcp::client::Client { rng, mac: conf.mac },
            socket: conf.socket,
            server_port: conf.server_port,
            timeout: conf.timeout,
            settings: None,
        }
    }

    /// Runs the DHCP client and takes care of all aspects of negotiating an IP with the first DHCP server that replies to the discovery requests.
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
    /// the operation of the client by just calling `run` later on. Of course, if the future is not polled, the client
    /// would be unable - during that time - to check for lease timeout and the lease might not be renewed on time.
    ///
    /// But in any case, if the lease is expired or the DHCP server does not acknowledge the lease renewal, the client will
    /// automatically restart the DHCP servers' discovery from the very beginning.
    pub async fn run(&mut self) -> Result<Option<Settings>, Error<F::Error>> {
        loop {
            if let Some((settings, acquired)) = self.settings.as_ref() {
                // Keep the lease
                let now = Instant::now();

                if now - *acquired
                    >= Duration::from_secs(settings.lease_time_secs.unwrap_or(7200) as u64 / 3)
                {
                    info!("Renewing DHCP lease...");

                    if let Some(settings) = self
                        .request(settings.server_ip.unwrap(), settings.ip)
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
                let offer = self.discover().await?;

                if let Some(settings) = self.request(offer.server_ip.unwrap(), offer.ip).await? {
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
    pub async fn release(&mut self) -> Result<(), Error<F::Error>> {
        if let Some((settings, _)) = self.settings.as_ref().cloned() {
            let server_ip = settings.server_ip.unwrap();
            let (_, mut socket) = self
                .stack
                .connect_from(
                    SocketAddr::V4(self.socket),
                    SocketAddr::V4(SocketAddrV4::new(server_ip, self.server_port)),
                )
                .await
                .map_err(Error::Io)?;

            let mut opt_buf = Options::buf();
            let request = self.client.release(&mut opt_buf, 0, settings.ip);

            socket
                .send(request.encode(self.buf)?)
                .await
                .map_err(Error::Io)?;
        }

        self.settings = None;

        Ok(())
    }

    async fn discover(&mut self) -> Result<Settings, Error<F::Error>> {
        info!("Discovering DHCP servers...");

        let start = Instant::now();

        loop {
            let mut socket = self
                .stack
                .bind_multiple(SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::UNSPECIFIED,
                    self.socket.port(),
                )))
                .await
                .map_err(Error::Io)?;

            let mut opt_buf = Options::buf();

            let (request, xid) =
                self.client
                    .discover(&mut opt_buf, (Instant::now() - start).as_secs() as _, None);

            socket
                .send(
                    SocketAddr::V4(self.socket),
                    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, self.server_port)),
                    request.encode(self.buf)?,
                )
                .await
                .map_err(Error::Io)?;

            let offer_start = Instant::now();

            while Instant::now() - offer_start < self.timeout {
                let timer = Timer::after(Duration::from_secs(3));

                if let Either::First(result) = select(socket.receive_into(self.buf), timer).await {
                    let (len, _local, _remote) = result.map_err(Error::Io)?;
                    let reply = Packet::decode(&self.buf[..len])?;

                    if self.client.is_offer(&reply, xid) {
                        let settings: Settings = (&reply).into();

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

    async fn request(
        &mut self,
        server_ip: Ipv4Addr,
        ip: Ipv4Addr,
    ) -> Result<Option<Settings>, Error<F::Error>> {
        for _ in 0..3 {
            info!("Requesting IP {ip} from DHCP server {server_ip}");

            let (_, mut socket) = self
                .stack
                .bind_single(SocketAddr::V4(self.socket))
                .await
                .map_err(Error::Io)?;

            let start = Instant::now();

            let mut opt_buf = Options::buf();

            let (request, xid) =
                self.client
                    .request(&mut opt_buf, (Instant::now() - start).as_secs() as _, ip);

            socket
                .send(
                    SocketAddr::V4(self.socket),
                    SocketAddr::V4(SocketAddrV4::new(server_ip, self.server_port)),
                    request.encode(self.buf)?,
                )
                .await
                .map_err(Error::Io)?;

            let request_start = Instant::now();

            while Instant::now() - request_start < self.timeout {
                let timer = Timer::after(Duration::from_secs(10));

                if let Either::First(result) = select(socket.receive_into(self.buf), timer).await {
                    let (len, _local, _remote) = result.map_err(Error::Io)?;
                    let packet = &self.buf[..len];

                    let reply = Packet::decode(packet)?;

                    if self.client.is_ack(&reply, xid) {
                        let settings = (&reply).into();

                        info!("IP {} leased successfully", ip);

                        return Ok(Some(settings));
                    } else if self.client.is_nak(&reply, xid) {
                        info!("IP {} not acknowledged", ip);

                        return Ok(None);
                    }
                }
            }

            drop(socket);
        }

        warn!("IP request was not replied");

        Ok(None)
    }
}
