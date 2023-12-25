use embedded_nal_async::Ipv4Addr;

use embedded_nal_async_xtra::UnconnectedUdpWithMac;

use log::{info, warn};

use self::dhcp::{Options, Packet};

pub use super::*;

/// Runs the provided DHCP server asynchronously using the supplied UDP socket and server options.
///
/// All incoming BOOTP requests are processed by updating the DHCP server's internal simple database of leases,
/// and by issuing replies.
///
/// Dropping this future is safe in that it won't remove the internal leases' database,
/// so users are free to drop the future in case they would like to take a snapshot of the leases or inspect them otherwise.
///
/// Note that the UDP socket that the server takes need to be capable of sending and receiving broadcast UDP packets.
///
/// Furthermore, some DHCP clients do send DHCP OFFER packets without the broadcast flag in the DHCP payload being set to true.
/// To support these clients, the socket needs to also be capable of sending packets with a broadcast IP destination address
/// - yet - with the destination *MAC* address in the Ethernet frame set to the MAC address of the DHCP client.
///
/// Such UDP sockets implement the `UnconnectedUdpWithMac` trait and are essentially based on the raw socket functionality,
/// as available on most operating systems.
pub async fn run<T, const N: usize>(
    server: &mut dhcp::server::Server<N>,
    server_options: &dhcp::server::ServerOptions<'_>,
    socket: &mut T,
    buf: &mut [u8],
) -> Result<(), Error<T::Error>>
where
    T: UnconnectedUdp + UnconnectedUdpWithMac,
{
    info!(
        "Running DHCP server for addresses {}-{} with configuration {server_options:?}",
        server.range_start, server.range_end
    );

    loop {
        let (len, local, remote) = UnconnectedUdp::receive_into(socket, buf)
            .await
            .map_err(Error::Io)?;
        let packet = &buf[..len];

        let request = match Packet::decode(packet) {
            Ok(request) => request,
            Err(err) => {
                warn!("Decoding packet returned error: {:?}", err);
                continue;
            }
        };

        let mut opt_buf = Options::buf();

        if let Some(reply) = server.handle_request(&mut opt_buf, server_options, &request) {
            let remote = if let SocketAddr::V4(socket) = remote {
                if request.broadcast || *socket.ip() == Ipv4Addr::UNSPECIFIED {
                    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, socket.port()))
                } else {
                    remote
                }
            } else {
                remote
            };

            let remote_mac = if request.broadcast {
                [0xff; 6]
            } else {
                request.chaddr[..6].try_into().unwrap()
            };

            UnconnectedUdpWithMac::send(
                socket,
                local,
                remote,
                Some(&remote_mac),
                reply.encode(buf)?,
            )
            .await
            .map_err(Error::Io)?;
        }
    }
}
