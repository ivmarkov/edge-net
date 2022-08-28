use embedded_io::blocking::Read;

pub fn try_read_full<R: Read>(mut read: R, buf: &mut [u8]) -> Result<usize, (R::Error, usize)> {
    let mut offset = 0;
    let mut size = 0;

    loop {
        let size_read = read.read(&mut buf[offset..]).map_err(|e| (e, size))?;

        offset += size_read;
        size += size_read;

        if size_read == 0 || size == buf.len() {
            break;
        }
    }

    Ok(size)
}

#[cfg(feature = "nightly")]
pub mod asynch {
    use embedded_io::asynch::Read;

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
}
