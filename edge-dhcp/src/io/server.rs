use core::fmt::Debug;

use embassy_time::Duration;

use embedded_nal_async::Ipv4Addr;

use log::{info, warn};

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

impl<'a> Configuration<'a> {
    pub fn new(ip: Ipv4Addr, gw_buf: Option<&'a mut [Ipv4Addr; 1]>) -> Self {
        let octets = ip.octets();

        let gateways = if let Some(gw_buf) = gw_buf {
            gw_buf[0] = ip;
            gw_buf.as_slice()
        } else {
            &[]
        };

        Self {
            socket: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_SERVER_PORT),
            ip,
            gateways,
            subnet: Some(Ipv4Addr::new(255, 255, 255, 0)),
            dns: &[],
            range_start: Ipv4Addr::new(octets[0], octets[1], octets[2], 50),
            range_end: Ipv4Addr::new(octets[0], octets[1], octets[2], 200),
            lease_duration_secs: 7200,
        }
    }
}

/// A simple asynchronous DHCP server.
///
/// The server takes a UDP socket stack and then processes all incoming BOOTP requests, by updating its internal simple database of leases,
/// and issuing replies.
///
/// Note that the `UdpStack` implementation that the server takes need to be a bit special, because DHCP clients operate
/// before they have an IP address assigned:
/// - It needs to be able to send broadcast packets (to IP 255.255.255.255)
/// - It needs to be able to receive broadcast packets
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

    /// Runs the DHCP server, processing incoming DHCP requests.
    ///
    /// Note that dropping this future is safe in that it won't remove the internal leases' database,
    /// so users are free to drop the future in case they would like to take a snapshot of the leases or inspect them otherwise.
    pub async fn run(&mut self) -> Result<(), Error<F::Error>> {
        let (_, mut socket) = self
            .stack
            .bind_single(SocketAddr::V4(self.socket))
            .await
            .map_err(Error::Io)?;

        loop {
            let (len, local, remote) = socket.receive_into(self.buf).await.map_err(Error::Io)?;
            let packet = &self.buf[..len];

            let request = match Packet::decode(packet) {
                Ok(request) => request,
                Err(err) => {
                    warn!("Decoding packet returned error: {:?}", err);
                    continue;
                }
            };

            let mut opt_buf = Options::buf();

            if let Some(request) =
                self.server
                    .handle_request(&mut opt_buf, &self.server_options, &request)
            {
                socket
                    .send(
                        local,
                        if true
                        // TODO: Why
                        /*request.broadcast*/
                        {
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
