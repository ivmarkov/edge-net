//! This module provides a utility `with_timeout(io, duration)` 
//! decorator function for IO types.
//!
//! Note that the presence of this module in the `edge-nal` crate
//! is a bit controversial, as it is a utility, while `edge-nal` is a
//! pure traits' crate otherwise.
//!
//! Therefore, the module might be moved to another location in future.

use embassy_time::{with_timeout as ewith_timeout, Duration, TimeoutError};

use embedded_io_async::{ErrorKind, ErrorType, Read, Write};

use crate::TcpShutdown;

/// IO Error type for the `with_timeout` function
#[derive(Debug)]
pub enum WithTimeoutError<E> {
    /// An IO error occurred
    IO(E),
    /// The operation timed out
    Timeout,
}

impl<E> From<E> for WithTimeoutError<E> {
    fn from(e: E) -> Self {
        Self::IO(e)
    }
}

impl<E> embedded_io_async::Error for WithTimeoutError<E>
where
    E: embedded_io_async::Error,
{
    fn kind(&self) -> ErrorKind {
        match self {
            Self::IO(e) => e.kind(),
            Self::Timeout => ErrorKind::TimedOut,
        }
    }
}

/// Add a timeout to all operations on the provided IO type,
/// where the operations are amongst the ones supported via the
/// `Read`, `Write`, and `TcpShutdown` traits.
///
/// Parameters:
/// - `io`: The IO type to add a timeout to
/// - `timeout`: The timeout duration
pub fn with_timeout<T>(io: T, timeout: Duration) -> WithTimeout<T> {
    WithTimeout::new(io, timeout)
}

/// A type that wraps an IO type and adds a timeout to all operations
pub struct WithTimeout<T>(T, Duration);

impl<T> WithTimeout<T> {
    const fn new(inner: T, timeout: Duration) -> Self {
        Self(inner, timeout)
    }
}

impl<T> ErrorType for WithTimeout<T>
where
    T: ErrorType,
{
    type Error = WithTimeoutError<T::Error>;
}

impl<T> Read for WithTimeout<T>
where
    T: Read,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        map_result(ewith_timeout(self.1, self.0.read(buf)).await)
    }
}

impl<T> Write for WithTimeout<T>
where
    T: Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        map_result(ewith_timeout(self.1, self.0.write(buf)).await)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        map_result(ewith_timeout(self.1, self.0.flush()).await)
    }
}

impl<T> TcpShutdown for WithTimeout<T>
where
    T: TcpShutdown,
{
    async fn close(&mut self, what: crate::Close) -> Result<(), Self::Error> {
        map_result(ewith_timeout(self.1, self.0.close(what)).await)
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        map_result(ewith_timeout(self.1, self.0.abort()).await)
    }
}

fn map_result<T, E>(
    result: Result<Result<T, E>, TimeoutError>,
) -> Result<T, WithTimeoutError<E>>
where
    E: embedded_io_async::Error,
{
    match result {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(e)) => Err(WithTimeoutError::IO(e)),
        Err(_) => Err(WithTimeoutError::Timeout),
    }
}
