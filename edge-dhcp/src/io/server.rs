use core::fmt::Debug;

use embassy_time::Duration;

use embedded_nal_async::Ipv4Addr;

use log::info;

use self::dhcp::{Options, Packet};

pub use super::*;

#[derive(Clone, Debug)]
pub struct Configuration<'a> {
    pub socket: SocketAddrV4,
    pub ip: Ipv4Addr,
    pub gateways: &'a [Ipv4Addr],
    pub subnet: Option<Ipv4Addr>,
    pub dns: &'a [Ipv4Addr],
    pub range_start: Ipv4Addr,
    pub range_end: Ipv4Addr,
    pub lease_duration_secs: u32,
}

/// A simple asynchronous DHCP server.
///
/// The client takes a socket factory (either operating on raw sockets or UDP datagrams) and
/// then processes all incoming BOOTP requests, by updating its internal simple database of leases, and issuing replies.
pub struct Server<'a, F, const N: usize = 64> {
    stack: F,
    buf: &'a mut [u8],
    socket: SocketAddrV4,
    server_options: dhcp::server::ServerOptions<'a>,
    pub server: dhcp::server::Server<N>,
}

impl<'a, F, const N: usize> Server<'a, F, N>
where
    F: UdpStack,
{
    pub fn new(stack: F, buf: &'a mut [u8], conf: &Configuration<'a>) -> Self {
        info!("Creating DHCP server with configuration {conf:?}");

        Self {
            stack,
            buf,
            socket: conf.socket,
            server_options: dhcp::server::ServerOptions {
                ip: conf.ip,
                gateways: conf.gateways,
                subnet: conf.subnet,
                dns: conf.dns,
                lease_duration: Duration::from_secs(conf.lease_duration_secs as _),
            },
            server: dhcp::server::Server {
                range_start: conf.range_start,
                range_end: conf.range_end,
                leases: heapless::LinearMap::new(),
            },
        }
    }

    /// Runs the DHCP server wth the supplied socket factory, processing incoming DHCP requests.
    ///
    /// Note that dropping this future is safe in that it won't remove the internal leases' database,
    /// so users are free to drop the future in case they would like to take a snapshot of the leases or inspect them otherwise.
    pub async fn run(&mut self) -> Result<(), Error<F::Error>> {
        let mut socket = self
            .stack
            .bind_multiple(SocketAddr::V4(self.socket))
            .await
            .map_err(Error::Io)?;

        loop {
            let (len, local, remote) = socket.receive_into(self.buf).await.map_err(Error::Io)?;
            let packet = &self.buf[..len];

            let request = Packet::decode(packet)?;

            let mut opt_buf = Options::buf();

            if let Some(request) =
                self.server
                    .handle_request(&mut opt_buf, &self.server_options, &request)
            {
                socket
                    .send(
                        local,
                        if request.broadcast {
                            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, remote.port()))
                        } else {
                            remote
                        },
                        request.encode(self.buf)?,
                    )
                    .await
                    .map_err(Error::Io)?;
            }
        }
    }
}
