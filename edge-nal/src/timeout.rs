//! This module provides utility function and a decorator struct
//! for adding timeouts to IO types.
//!
//! Note that the presence of this module in the `edge-nal` crate
//! is a bit controversial, as it is a utility, while `edge-nal` is a
//! pure traits' crate otherwise.
//!
//! Therefore, the module might be moved to another location in future.

use core::{
    fmt::{self, Display},
    future::Future,
    net::SocketAddr,
};

use embassy_time::Duration;
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};

use crate::{Readable, TcpConnect, TcpShutdown};

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

impl<E> fmt::Display for WithTimeoutError<E>
where
    E: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IO(e) => write!(f, "IO error: {}", e),
            Self::Timeout => write!(f, "Operation timed out"),
        }
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
/// - `timeout_ms`: The timeout duration in milliseconds
/// - `fut`: The future to run
pub async fn with_timeout<F, T, E>(timeout_ms: u32, fut: F) -> Result<T, WithTimeoutError<E>>
where
    F: Future<Output = Result<T, E>>,
    E: embedded_io_async::Error,
{
    map_result(embassy_time::with_timeout(Duration::from_millis(timeout_ms as _), fut).await)
}

/// A type that wraps an IO stream type and adds a timeout to all operations.
///
/// The operations decorated with a timeout are the ones offered via the following traits:
/// - `embedded_io_async::Read`
/// - `embedded_io_async::Write`
/// - `Readable`
/// - `TcpConnect`
/// - `TcpShutdown`
pub struct WithTimeout<T>(T, u32);

impl<T> WithTimeout<T> {
    /// Create a new `WithTimeout` instance.
    ///
    /// Parameters:
    /// - `timeout_ms`: The timeout duration in milliseconds
    /// - `io`: The IO type to add a timeout to
    pub const fn new(timeout_ms: u32, io: T) -> Self {
        Self(io, timeout_ms)
    }

    /// Get a reference to the inner IO type.
    pub fn io(&self) -> &T {
        &self.0
    }

    /// Get a mutable reference to the inner IO type.
    pub fn io_mut(&mut self) -> &mut T {
        &mut self.0
    }

    /// Get the IO type by destructuring the `WithTimeout` instance.
    pub fn into_io(self) -> T {
        self.0
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

impl<T> TcpConnect for WithTimeout<T>
where
    T: TcpConnect,
{
    type Error = WithTimeoutError<T::Error>;

    type Socket<'a>
        = WithTimeout<T::Socket<'a>>
    where
        Self: 'a;

    async fn connect(&self, remote: SocketAddr) -> Result<Self::Socket<'_>, Self::Error> {
        with_timeout(self.1, self.0.connect(remote))
            .await
            .map(|s| WithTimeout::new(self.1, s))
    }
}

impl<T> Readable for WithTimeout<T>
where
    T: Readable,
{
    async fn readable(&mut self) -> Result<(), Self::Error> {
        with_timeout(self.1, self.0.readable()).await
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
