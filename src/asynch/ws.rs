use core::cmp::min;

use embedded_io::asynch::{Read, ReadExactError, Write};

pub type Fragmented = bool;
pub type Final = bool;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FrameType {
    Text(Fragmented),
    Binary(Fragmented),
    Ping,
    Pong,
    Close,
    Continue(Final),
}

impl FrameType {
    pub fn is_fragmented(&self) -> bool {
        match self {
            Self::Text(fragmented) | Self::Binary(fragmented) => *fragmented,
            Self::Continue(_) => true,
            _ => false,
        }
    }

    pub fn is_final(&self) -> bool {
        match self {
            Self::Text(fragmented) | Self::Binary(fragmented) => !*fragmented,
            Self::Continue(final_) => *final_,
            _ => true,
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum DeserializeError {
    Incomplete(usize),
    Invalid,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum SerializeError {
    TooShort,
    TooLong,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Error<E> {
    Deserialize(DeserializeError),
    Serialize(SerializeError),
    Io(E),
}

impl<E> From<ReadExactError<E>> for Error<E> {
    fn from(e: ReadExactError<E>) -> Self {
        match e {
            ReadExactError::UnexpectedEof => Error::Deserialize(DeserializeError::Invalid),
            ReadExactError::Other(e) => Error::Io(e),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrameHeader {
    pub frame_type: FrameType,
    pub payload_len: usize,
    pub mask_key: Option<u32>,
}

impl FrameHeader {
    pub const MIN_LEN: usize = 2;
    pub const MAX_LEN: usize = FrameHeader {
        frame_type: FrameType::Binary(false),
        payload_len: 65536,
        mask_key: Some(0),
    }
    .serialized_len();

    pub fn deserialize(buf: &[u8]) -> Result<(Self, usize), DeserializeError> {
        let mut expected_len = 2_usize;

        if buf.len() < expected_len {
            Err(DeserializeError::Incomplete(expected_len - buf.len()))
        } else {
            let final_frame = buf[0] & 0x80 != 0;

            let rsv = buf[0] & 0x70;
            if rsv != 0 {
                return Err(DeserializeError::Invalid);
            }

            let opcode = buf[0] & 0x0f;
            if (3..=7).contains(&opcode) || opcode >= 11 {
                return Err(DeserializeError::Invalid);
            }

            let masked = buf[1] & 0x80 != 0;
            if masked {
                expected_len += 4;
            }

            let mut payload_len = buf[1] as usize & 0x7f;
            let mut payload_offset = 2;

            if payload_len == 126 {
                expected_len += 2;

                if buf.len() < expected_len {
                    return Err(DeserializeError::Incomplete(expected_len - buf.len()));
                } else {
                    payload_len = ((buf[2] as usize) << 8) | buf[3] as usize;
                    payload_offset += 2;
                }
            } else if payload_len == 127 {
                expected_len += 3;

                if buf.len() < expected_len {
                    return Err(DeserializeError::Incomplete(5 - buf.len()));
                } else {
                    payload_len =
                        ((buf[2] as usize) << 16) | ((buf[3] as usize) << 8) | buf[4] as usize;
                    payload_offset += 3;
                }
            }

            let masked = buf[1] & 0x80 != 0;
            let mask_key = if masked {
                let mask_key = Some(u32::from_be_bytes([
                    buf[payload_offset],
                    buf[payload_offset + 1],
                    buf[payload_offset + 2],
                    buf[payload_offset + 3],
                ]));
                payload_offset += 4;

                mask_key
            } else {
                None
            };

            let frame_header = FrameHeader {
                frame_type: match opcode {
                    0 => FrameType::Continue(final_frame),
                    1 => FrameType::Text(!final_frame),
                    2 => FrameType::Binary(!final_frame),
                    8 => FrameType::Close,
                    9 => FrameType::Ping,
                    10 => FrameType::Pong,
                    _ => unreachable!(),
                },
                payload_len,
                mask_key,
            };

            Ok((frame_header, payload_offset))
        }
    }

    pub const fn serialized_len(&self) -> usize {
        let len = if self.payload_len >= 65536 {
            3
        } else if self.payload_len > 126 {
            2
        } else {
            1
        };

        2 + if self.mask_key.is_some() { 4 } else { 0 } + len
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, SerializeError> {
        if buf.len() < self.serialized_len() {
            return Err(SerializeError::TooShort);
        }

        buf[0] = 0;
        buf[1] = 0;

        if self.frame_type.is_final() {
            buf[0] |= 0x80;
        }

        let opcode = match self.frame_type {
            FrameType::Text(_) => 1,
            FrameType::Binary(_) => 2,
            FrameType::Close => 3,
            FrameType::Ping => 4,
            FrameType::Pong => 5,
            _ => 0,
        };

        buf[0] |= opcode;

        let mut payload_offset = 2;

        if self.payload_len < 126 {
            buf[1] |= self.payload_len as u8;
        } else {
            let payload_len_bytes = self.payload_len.to_be_bytes();
            if self.payload_len > 126 && self.payload_len < 65536 {
                buf[2] = payload_len_bytes[2];
                buf[3] = payload_len_bytes[3];

                payload_offset += 2;
            } else if self.payload_len < 0xffffff {
                buf[2] = payload_len_bytes[1];
                buf[3] = payload_len_bytes[2];
                buf[4] = payload_len_bytes[3];

                payload_offset += 3;
            } else {
                return Err(SerializeError::TooLong);
            }
        }

        if let Some(mask_key) = self.mask_key {
            buf[1] |= 0x80;

            let mask_key_bytes = mask_key.to_be_bytes();

            buf[payload_offset] = mask_key_bytes[0];
            buf[payload_offset + 1] = mask_key_bytes[1];
            buf[payload_offset + 2] = mask_key_bytes[2];
            buf[payload_offset + 3] = mask_key_bytes[3];

            payload_offset += 4;
        }

        Ok(payload_offset)
    }

    pub fn mask(&self, buf: &mut [u8], payload_offset: usize) {
        if let Some(mask_key) = self.mask_key {
            let mask_bytes = mask_key.to_be_bytes();

            for (offset, byte) in buf.iter_mut().enumerate() {
                *byte ^= mask_bytes[(payload_offset + offset) % 4];
            }
        }
    }

    pub async fn recv<R>(mut read: R) -> Result<Self, Error<R::Error>>
    where
        R: Read,
    {
        let mut header_buf = [0; FrameHeader::MAX_LEN];

        read.read_exact(&mut header_buf[..FrameHeader::MIN_LEN])
            .await
            .map_err(Error::from)?;

        match FrameHeader::deserialize(&header_buf[..FrameHeader::MIN_LEN]) {
            Ok((header, _)) => Ok(header),
            Err(DeserializeError::Incomplete(more)) => {
                let header_len = FrameHeader::MIN_LEN + more;
                read.read_exact(&mut header_buf[FrameHeader::MIN_LEN..header_len])
                    .await
                    .map_err(Error::from)?;

                match FrameHeader::deserialize(&header_buf[..header_len]) {
                    Ok((header, header_len2)) => {
                        if header_len != header_len2 {
                            unreachable!();
                        }

                        Ok(header)
                    }
                    Err(DeserializeError::Incomplete(_)) => unreachable!(),
                    Err(err) => Err(Error::Deserialize(err)),
                }
            }
            Err(DeserializeError::Invalid) => Err(Error::Deserialize(DeserializeError::Invalid)),
        }
    }

    pub async fn send<W>(&self, mut write: W) -> Result<(), Error<W::Error>>
    where
        W: Write,
    {
        let mut header_buf = [0; FrameHeader::MAX_LEN];
        let header_len = self.serialize(&mut header_buf).unwrap();

        write
            .write_all(&header_buf[..header_len])
            .await
            .map_err(Error::Io)
    }

    pub async fn recv_payload<'a, R>(
        &'a self,
        mut read: R,
        payload: &'a mut [u8],
    ) -> Result<(), Error<R::Error>>
    where
        R: Read,
    {
        if payload.len() < self.payload_len {
            Err(Error::Serialize(SerializeError::TooShort))
        } else if payload.len() > self.payload_len {
            Err(Error::Serialize(SerializeError::TooLong))
        } else if payload.is_empty() {
            Ok(())
        } else {
            read.read_exact(payload).await.map_err(Error::from)?;

            self.mask(payload, 0);

            Ok(())
        }
    }

    pub async fn send_payload<'a, W>(
        &'a self,
        mut write: W,
        payload: &'a [u8],
    ) -> Result<(), Error<W::Error>>
    where
        W: Write,
    {
        if payload.len() < self.payload_len {
            Err(Error::Serialize(SerializeError::TooShort))
        } else if payload.len() > self.payload_len {
            Err(Error::Serialize(SerializeError::TooLong))
        } else if payload.is_empty() {
            Ok(())
        } else if self.mask_key.is_none() {
            write.write_all(payload).await.map_err(Error::Io)
        } else {
            let mut buf = [0_u8; 64];

            let mut offset = 0;

            while offset < payload.len() {
                let len = min(buf.len(), payload.len() - offset);

                buf[..len].copy_from_slice(&payload[offset..offset + len]);

                self.mask(&mut buf, offset);

                write.write_all(&buf).await.map_err(Error::Io)?;

                offset += len;
            }

            Ok(())
        }
    }
}

pub async fn recv<R>(
    mut read: R,
    frame_data_buf: &mut [u8],
) -> Result<(FrameType, usize), Error<R::Error>>
where
    R: Read,
{
    let header = FrameHeader::recv(&mut read).await?;
    header.recv_payload(read, frame_data_buf).await?;

    Ok((header.frame_type, header.payload_len))
}

pub async fn send<W>(
    mut write: W,
    frame_type: FrameType,
    mask_key: Option<u32>,
    frame_data_buf: &[u8],
) -> Result<(), Error<W::Error>>
where
    W: Write,
{
    let header = FrameHeader {
        frame_type,
        payload_len: frame_data_buf.len(),
        mask_key,
    };

    header.send(&mut write).await?;
    header.send_payload(write, frame_data_buf).await
}

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::convert::{TryFrom, TryInto};
    use core::future::Future;

    use embedded_io::asynch::{Read, Write};
    use embedded_svc::io::Io;
    use embedded_svc::ws::asynch::Sender;
    use embedded_svc::ws::ErrorType;
    use embedded_svc::ws::{asynch::Receiver, FrameType};

    use super::Error;

    impl From<super::FrameType> for FrameType {
        fn from(frame_type: super::FrameType) -> Self {
            match frame_type {
                super::FrameType::Text(v) => Self::Text(v),
                super::FrameType::Binary(v) => Self::Binary(v),
                super::FrameType::Ping => Self::Ping,
                super::FrameType::Pong => Self::Pong,
                super::FrameType::Close => Self::Close,
                super::FrameType::Continue(v) => Self::Continue(v),
            }
        }
    }

    impl TryFrom<FrameType> for super::FrameType {
        type Error = FrameType;

        fn try_from(frame_type: FrameType) -> Result<Self, Self::Error> {
            let f = match frame_type {
                FrameType::Text(v) => Self::Text(v),
                FrameType::Binary(v) => Self::Binary(v),
                FrameType::Ping => Self::Ping,
                FrameType::Pong => Self::Pong,
                FrameType::Close => Self::Close,
                FrameType::SocketClose => Err(FrameType::SocketClose)?,
                FrameType::Continue(v) => Self::Continue(v),
            };

            Ok(f)
        }
    }

    pub struct WsConnection<T, M>(T, M);

    impl<T, M> ErrorType for WsConnection<T, M>
    where
        T: Io,
    {
        type Error = Error<T::Error>;
    }

    impl<T, M> Receiver for WsConnection<T, M>
    where
        T: Read,
    {
        type ReceiveFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(FrameType, usize), Self::Error>>;

        fn recv<'a>(&'a mut self, frame_data_buf: &'a mut [u8]) -> Self::ReceiveFuture<'a> {
            async move {
                super::recv(&mut self.0, frame_data_buf)
                    .await
                    .map(|(frame_type, payload_len)| (frame_type.into(), payload_len))
            }
        }
    }

    impl<T, M> Sender for WsConnection<T, M>
    where
        T: Write,
        M: Fn() -> Option<u32>,
    {
        type SendFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        fn send<'a>(
            &'a mut self,
            frame_type: FrameType,
            frame_data: &'a [u8],
        ) -> Self::SendFuture<'a> {
            async move {
                super::send(
                    &mut self.0,
                    frame_type.try_into().unwrap(),
                    (self.1)(),
                    frame_data,
                )
                .await
            }
        }
    }
}
