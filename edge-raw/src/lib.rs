#![cfg_attr(not(feature = "std"), no_std)]
#![allow(stable_features)]
#![allow(unknown_lints)]
#![cfg_attr(feature = "nightly", feature(async_fn_in_trait))]
#![cfg_attr(feature = "nightly", allow(async_fn_in_trait))]
#![cfg_attr(feature = "nightly", feature(impl_trait_projections))]

use no_std_net::{Ipv4Addr, SocketAddrV4};

use self::udp::UdpPacketHeader;

#[cfg(feature = "nightly")]
pub mod io;

pub mod bytes;
pub mod ip;
pub mod udp;

use bytes::BytesIn;

#[derive(Debug)]
pub enum Error {
    DataUnderflow,
    BufferOverflow,
    InvalidFormat,
    InvalidChecksum,
}

impl From<bytes::Error> for Error {
    fn from(value: bytes::Error) -> Self {
        match value {
            bytes::Error::BufferOverflow => Self::BufferOverflow,
            bytes::Error::DataUnderflow => Self::DataUnderflow,
            bytes::Error::InvalidFormat => Self::InvalidFormat,
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn ip_udp_decode(
    packet: &[u8],
    filter_src: Option<SocketAddrV4>,
    filter_dst: Option<SocketAddrV4>,
) -> Result<Option<(SocketAddrV4, SocketAddrV4, &[u8])>, Error> {
    if let Some((src, dst, _proto, udp_packet)) = ip::decode(
        packet,
        filter_src.map(|a| *a.ip()).unwrap_or(Ipv4Addr::UNSPECIFIED),
        filter_dst.map(|a| *a.ip()).unwrap_or(Ipv4Addr::UNSPECIFIED),
        Some(UdpPacketHeader::PROTO),
    )? {
        udp::decode(
            src,
            dst,
            udp_packet,
            filter_src.map(|a| a.port()),
            filter_dst.map(|a| a.port()),
        )
    } else {
        Ok(None)
    }
}

pub fn ip_udp_encode<F>(
    buf: &mut [u8],
    src: SocketAddrV4,
    dst: SocketAddrV4,
    encoder: F,
) -> Result<&[u8], Error>
where
    F: FnOnce(&mut [u8]) -> Result<usize, Error>,
{
    ip::encode(buf, *src.ip(), *dst.ip(), UdpPacketHeader::PROTO, |buf| {
        Ok(udp::encode(buf, src, dst, encoder)?.len())
    })
}

pub fn checksum_accumulate(bytes: &[u8], checksum_word: usize) -> u32 {
    let mut bytes = BytesIn::new(bytes);

    let mut sum: u32 = 0;
    while !bytes.is_empty() {
        let skip = (bytes.offset() >> 1) == checksum_word;
        let arr = bytes
            .arr()
            .ok()
            .unwrap_or_else(|| [bytes.byte().unwrap(), 0]);

        let word = if skip { 0 } else { u16::from_be_bytes(arr) };

        sum += word as u32;
    }

    sum
}

pub fn checksum_finish(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum >> 16) + (sum & 0xffff);
    }

    !sum as u16
}
