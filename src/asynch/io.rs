use embedded_io::Error;
use embedded_io_async::{Read, Write};

pub async fn try_read_full<R: Read>(
    mut read: R,
    buf: &mut [u8],
) -> Result<usize, (R::Error, usize)> {
    let mut offset = 0;
    let mut size = 0;

    loop {
        let size_read = read.read(&mut buf[offset..]).await.map_err(|e| (e, size))?;

        offset += size_read;
        size += size_read;

        if size_read == 0 || size == buf.len() {
            break;
        }
    }

    Ok(size)
}

#[derive(Debug)]
pub enum CopyError<R, W> {
    Read(R),
    Write(W),
}

impl<R, W> Error for CopyError<R, W>
where
    R: Error,
    W: Error,
{
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Self::Read(e) => e.kind(),
            Self::Write(e) => e.kind(),
        }
    }
}

pub async fn copy<const N: usize, R, W>(
    read: R,
    write: W,
) -> Result<u64, CopyError<R::Error, W::Error>>
where
    R: Read,
    W: Write,
{
    copy_len::<N, _, _>(read, write, u64::MAX).await
}

pub async fn copy_len<const N: usize, R, W>(
    read: R,
    write: W,
    len: u64,
) -> Result<u64, CopyError<R::Error, W::Error>>
where
    R: Read,
    W: Write,
{
    copy_len_with_progress::<N, _, _, _>(read, write, len, |_, _| {}).await
}

pub async fn copy_len_with_progress<const N: usize, R, W, P>(
    mut read: R,
    mut write: W,
    mut len: u64,
    progress: P,
) -> Result<u64, CopyError<R::Error, W::Error>>
where
    R: Read,
    W: Write,
    P: Fn(u64, u64),
{
    let mut buf = [0_u8; N];

    let mut copied = 0;

    while len > 0 {
        progress(copied, len);

        let size_read = read.read(&mut buf).await.map_err(CopyError::Read)?;
        if size_read == 0 {
            break;
        }

        write
            .write_all(&buf[0..size_read])
            .await
            .map_err(map_write_err)
            .map_err(CopyError::Write)?;

        copied += size_read as u64;
        len -= size_read as u64;
    }

    progress(copied, len);

    Ok(copied)
}

pub(crate) fn map_write_err<W>(e: embedded_io::WriteAllError<W>) -> W {
    match e {
        embedded_io::WriteAllError::WriteZero => panic!("write() returned Ok(0)"),
        embedded_io::WriteAllError::Other(e) => e,
    }
}
