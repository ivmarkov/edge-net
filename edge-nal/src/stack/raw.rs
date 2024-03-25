//! Factory traits for creating raw sockets on embedded devices

use embedded_io_async::ErrorType;

use crate::raw::{RawReceive, RawSend};

/// This trait is implemented by raw sockets that can be split into separate `send` and `receive` halves that can operate
/// independently from each other (i.e., a full-duplex connection).
///
/// All sockets returned by the `RawStack` trait must implement this trait.
pub trait RawSplit: ErrorType {
    type Receive<'a>: RawReceive<Error = Self::Error>
    where
        Self: 'a;
    type Send<'a>: RawSend<Error = Self::Error>
    where
        Self: 'a;

    fn split(&mut self) -> (Self::Receive<'_>, Self::Send<'_>);
}

impl<T> RawSplit for &mut T
where
    T: RawSplit,
{
    type Receive<'a> = T::Receive<'a> where Self: 'a;
    type Send<'a> = T::Send<'a> where Self: 'a;

    fn split(&mut self) -> (Self::Receive<'_>, Self::Send<'_>) {
        (**self).split()
    }
}

/// This trait is implemented by raw socket stacks. The trait allows the underlying driver to
/// construct multiple connections that implement the raw socket traits from `edge-net`.
pub trait RawStack {
    /// Error type returned on socket creation failure.
    type Error: embedded_io_async::Error;

    /// The socket type returned by the stack.
    type Socket<'a>: RawReceive<Error = Self::Error>
        + RawSend<Error = Self::Error>
        + RawSplit<Error = Self::Error>
    where
        Self: 'a;

    /// Create a raw socket.
    ///
    /// On most operating systems, creating a raw socket requires admin privileges.
    async fn bind(&self) -> Result<Self::Socket<'_>, Self::Error>;
}

impl<T> RawStack for &T
where
    T: RawStack,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn bind(&self) -> Result<Self::Socket<'_>, Self::Error> {
        (*self).bind().await
    }
}

impl<T> RawStack for &mut T
where
    T: RawStack,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn bind(&self) -> Result<Self::Socket<'_>, Self::Error> {
        (**self).bind().await
    }
}
