//! This module provides utility function and a decorator struct
//! for adding timeouts to IO types.
//!
//! Note that the presence of this module in the `edge-nal` crate
//! is a bit controversial, as it is a utility, while `edge-nal` is a
//! pure traits' crate otherwise.
//!
//! Therefore, the module might be moved to another location in future.

use core::future::Future;

use embedded_io_async::{ErrorKind, ErrorType, Read, Write};

pub use embassy_time::Duration;

use crate::TcpShutdown;

/// IO Error type for the `with_timeout` function and `WithTimeout` struct.
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

/// Run an IO future with a timeout.
///
/// A future is an IO future if it resolves to a `Result<T, E>`, where `E`
/// implements `embedded_io_async::Error`.
///
/// If the future completes before the timeout, its output is returned.
/// Otherwise, on timeout, a timeout error is returned.
///
/// Parameters:
/// - `timeout`: The timeout duration
/// - `fut`: The future to run
pub async fn with_timeout<F, T, E>(timeout: Duration, fut: F) -> Result<T, WithTimeoutError<E>>
where
    F: Future<Output = Result<T, E>>,
    E: embedded_io_async::Error,
{
    map_result(embassy_time::with_timeout(timeout, fut).await)
}

/// A type that wraps an IO stream type and adds a timeout to all operations.
///
/// The operations decorated with a timeout are the ones offered via the following traits:
/// - `embedded_io_async::Read`
/// - `embedded_io_async::Write`
/// - `TcpShutdown`
pub struct WithTimeout<T>(T, Duration);

impl<T> WithTimeout<T> {
    /// Create a new `WithTimeout` instance.
    ///
    /// Parameters:
    /// - `timeout`: The timeout duration
    /// - `io`: The IO type to add a timeout to
    pub const fn new(timeout: Duration, io: T) -> Self {
        Self(io, timeout)
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
        with_timeout(self.1, self.0.read(buf)).await
    }
}

impl<T> Write for WithTimeout<T>
where
    T: Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        with_timeout(self.1, self.0.write(buf)).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        with_timeout(self.1, self.0.flush()).await
    }
}

impl<T> TcpShutdown for WithTimeout<T>
where
    T: TcpShutdown,
{
    async fn close(&mut self, what: crate::Close) -> Result<(), Self::Error> {
        with_timeout(self.1, self.0.close(what)).await
    }

    async fn abort(&mut self) -> Result<(), Self::Error> {
        with_timeout(self.1, self.0.abort()).await
    }
}

fn map_result<T, E>(
    result: Result<Result<T, E>, embassy_time::TimeoutError>,
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
