use serde::{de::DeserializeOwned, Deserialize, Serialize};

use embedded_io::blocking::{Read, Write};

use super::io::*;

#[derive(Debug)]
pub enum SerdeError<E> {
    IoError(E),
    SerdeError,
}

pub fn read_buf<'a, R, T>(read: R, buf: &'a mut [u8]) -> Result<T, SerdeError<R::Error>>
where
    R: Read,
    T: Deserialize<'a>,
{
    let read_len = try_read_full(read, buf).map_err(|(e, _)| SerdeError::IoError(e))?;

    let (result, _) =
        serde_json_core::from_slice(&buf[..read_len]).map_err(|_| SerdeError::SerdeError)?;

    Ok(result)
}

pub fn read<const N: usize, R, T>(read: R) -> Result<T, SerdeError<R::Error>>
where
    R: Read,
    T: DeserializeOwned,
{
    let mut buf = [0_u8; N];

    let read_len = try_read_full(read, &mut buf).map_err(|(e, _)| SerdeError::IoError(e))?;

    let (result, _) =
        serde_json_core::from_slice(&buf[..read_len]).map_err(|_| SerdeError::SerdeError)?;

    Ok(result)
}

pub fn write_buf<W, T>(mut write: W, value: &T, buf: &mut [u8]) -> Result<(), SerdeError<W::Error>>
where
    W: Write,
    T: Serialize,
{
    let size = serde_json_core::to_slice(value, buf).map_err(|_| SerdeError::SerdeError)?;

    write.write_all(&buf[..size]).map_err(SerdeError::IoError)
}

pub fn write<const N: usize, W, T>(write: W, value: &T) -> Result<(), SerdeError<W::Error>>
where
    W: Write,
    T: Serialize,
{
    let mut buf = [0_u8; N];

    write_buf(write, value, &mut buf)
}

// pub fn response<const N: usize, C, T>(
//     request: crate::http::server::Request<C>,
//     value: &T,
// ) -> Result<(), SerdeError<C::Error>>
// where
//     C: crate::http::server::Connection,
//     T: Serialize,
// {
//     use crate::http::headers::content_type;

//     let mut response = request
//         .into_response(200, None, &[content_type("application/json")])
//         .map_err(SerdeError::IoError)?;

//     write::<N, _, _>(&mut response, value)?;

//     Ok(())
// }

#[cfg(feature = "nightly")]
pub mod asynch {
    use serde::{de::DeserializeOwned, Deserialize, Serialize};

    use embedded_io::asynch::{Read, Write};

    use crate::utils::io::asynch::*;

    pub use super::SerdeError;

    pub async fn read_buf<'a, R, T>(read: R, buf: &'a mut [u8]) -> Result<T, SerdeError<R::Error>>
    where
        R: Read,
        T: Deserialize<'a>,
    {
        let read_len = try_read_full(read, buf)
            .await
            .map_err(|(e, _)| SerdeError::IoError(e))?;

        let (result, _) =
            serde_json_core::from_slice(&buf[..read_len]).map_err(|_| SerdeError::SerdeError)?;

        Ok(result)
    }

    pub async fn read<const N: usize, R, T>(read: R) -> Result<T, SerdeError<R::Error>>
    where
        R: Read,
        T: DeserializeOwned,
    {
        let mut buf = [0_u8; N];

        let read_len = try_read_full(read, &mut buf)
            .await
            .map_err(|(e, _)| SerdeError::IoError(e))?;

        let (result, _) =
            serde_json_core::from_slice(&buf[..read_len]).map_err(|_| SerdeError::SerdeError)?;

        Ok(result)
    }

    pub async fn write_buf<W, T>(
        mut write: W,
        value: &T,
        buf: &mut [u8],
    ) -> Result<(), SerdeError<W::Error>>
    where
        W: Write,
        T: Serialize,
    {
        let size = serde_json_core::to_slice(value, buf).map_err(|_| SerdeError::SerdeError)?;

        write
            .write_all(&buf[..size])
            .await
            .map_err(SerdeError::IoError)
    }

    pub async fn write<const N: usize, W, T>(
        write: W,
        value: &T,
    ) -> Result<(), SerdeError<W::Error>>
    where
        W: Write,
        T: Serialize,
    {
        let mut buf = [0_u8; N];

        write_buf(write, value, &mut buf).await
    }
}
