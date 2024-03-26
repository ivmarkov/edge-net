use log::trace;

use core::net::{Ipv4Addr, SocketAddrV4};

use super::bytes::{BytesIn, BytesOut};

use super::{checksum_accumulate, checksum_finish, Error};

#[allow(clippy::type_complexity)]
pub fn decode(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    packet: &[u8],
    filter_src: Option<u16>,
    filter_dst: Option<u16>,
) -> Result<Option<(SocketAddrV4, SocketAddrV4, &[u8])>, Error> {
    let data = UdpPacketHeader::decode_with_payload(packet, src, dst, filter_src, filter_dst)?.map(
        |(hdr, payload)| {
            (
                SocketAddrV4::new(src, hdr.src),
                SocketAddrV4::new(dst, hdr.dst),
                payload,
            )
        },
    );

    Ok(data)
}

pub fn encode<F>(
    buf: &mut [u8],
    src: SocketAddrV4,
    dst: SocketAddrV4,
    payload: F,
) -> Result<&[u8], Error>
where
    F: FnOnce(&mut [u8]) -> Result<usize, Error>,
{
    let mut hdr = UdpPacketHeader::new(src.port(), dst.port());

    hdr.encode_with_payload(buf, *src.ip(), *dst.ip(), |buf| payload(buf))
}

/// Represents a parsed UDP header
#[derive(Clone, Debug)]
pub struct UdpPacketHeader {
    /// Source port
    pub src: u16,
    /// Destination port
    pub dst: u16,
    /// UDP length
    pub len: u16,
    /// UDP checksum
    pub sum: u16,
}

impl UdpPacketHeader {
    pub const PROTO: u8 = 17;

    pub const SIZE: usize = 8;
    pub const CHECKSUM_WORD: usize = 3;

    /// Create a new header instance
    pub fn new(src: u16, dst: u16) -> Self {
        Self {
            src,
            dst,
            len: 0,
            sum: 0,
        }
    }

    /// Decodes the header from a byte slice
    pub fn decode(data: &[u8]) -> Result<Self, Error> {
        let mut bytes = BytesIn::new(data);

        Ok(Self {
            src: u16::from_be_bytes(bytes.arr()?),
            dst: u16::from_be_bytes(bytes.arr()?),
            len: u16::from_be_bytes(bytes.arr()?),
            sum: u16::from_be_bytes(bytes.arr()?),
        })
    }

    /// Encodes the header into the provided buf slice
    pub fn encode<'o>(&self, buf: &'o mut [u8]) -> Result<&'o [u8], Error> {
        let mut bytes = BytesOut::new(buf);

        bytes
            .push(&u16::to_be_bytes(self.src))?
            .push(&u16::to_be_bytes(self.dst))?
            .push(&u16::to_be_bytes(self.len))?
            .push(&u16::to_be_bytes(self.sum))?;

        let len = bytes.len();

        Ok(&buf[..len])
    }

    /// Encodes the header and the provided payload into the provided buf slice
    pub fn encode_with_payload<'o, F>(
        &mut self,
        buf: &'o mut [u8],
        src: Ipv4Addr,
        dst: Ipv4Addr,
        encoder: F,
    ) -> Result<&'o [u8], Error>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, Error>,
    {
        if buf.len() < Self::SIZE {
            Err(Error::BufferOverflow)?;
        }

        let (hdr_buf, payload_buf) = buf.split_at_mut(Self::SIZE);

        let payload_len = encoder(payload_buf)?;

        let len = Self::SIZE + payload_len;
        self.len = len as _;

        let hdr_len = self.encode(hdr_buf)?.len();
        assert_eq!(Self::SIZE, hdr_len);

        let packet = &mut buf[..len];

        let checksum = Self::checksum(packet, src, dst);
        self.sum = checksum;

        Self::inject_checksum(packet, checksum);

        Ok(packet)
    }

    /// Decodes the provided packet into a header and a payload slice
    pub fn decode_with_payload(
        packet: &[u8],
        src: Ipv4Addr,
        dst: Ipv4Addr,
        filter_src: Option<u16>,
        filter_dst: Option<u16>,
    ) -> Result<Option<(Self, &[u8])>, Error> {
        let hdr = Self::decode(packet)?;

        if let Some(filter_src) = filter_src {
            if filter_src != hdr.src {
                return Ok(None);
            }
        }

        if let Some(filter_dst) = filter_dst {
            if filter_dst != hdr.dst {
                return Ok(None);
            }
        }

        let len = hdr.len as usize;
        if packet.len() < len {
            Err(Error::DataUnderflow)?;
        }

        let checksum = Self::checksum(&packet[..len], src, dst);

        trace!(
            "UDP header decoded, src={}, dst={}, size={}, checksum={}, ours={}",
            hdr.src,
            hdr.dst,
            hdr.len,
            hdr.sum,
            checksum
        );

        if checksum != hdr.sum {
            Err(Error::InvalidChecksum)?;
        }

        let packet = &packet[..len];

        let payload_data = &packet[Self::SIZE..];

        Ok(Some((hdr, payload_data)))
    }

    /// Injects the checksum into the provided packet
    pub fn inject_checksum(packet: &mut [u8], checksum: u16) {
        let checksum = checksum.to_be_bytes();

        let offset = Self::CHECKSUM_WORD << 1;
        packet[offset] = checksum[0];
        packet[offset + 1] = checksum[1];
    }

    /// Computes the checksum for an already encoded packet
    pub fn checksum(packet: &[u8], src: Ipv4Addr, dst: Ipv4Addr) -> u16 {
        let mut buf = [0; 12];

        // Pseudo IP-header for UDP checksum calculation
        let len = BytesOut::new(&mut buf)
            .push(&u32::to_be_bytes(src.into()))
            .unwrap()
            .push(&u32::to_be_bytes(dst.into()))
            .unwrap()
            .byte(0)
            .unwrap()
            .byte(UdpPacketHeader::PROTO)
            .unwrap()
            .push(&u16::to_be_bytes(packet.len() as u16))
            .unwrap()
            .len();

        let sum = checksum_accumulate(&buf[..len], usize::MAX)
            + checksum_accumulate(packet, Self::CHECKSUM_WORD);

        checksum_finish(sum)
    }
}
