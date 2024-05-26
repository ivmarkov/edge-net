//! Factory traits for creating raw sockets on embedded devices

use embedded_io_async::ErrorType;

use crate::raw::{RawReceive, RawSend};
use crate::Readable;

/// This trait is implemented by raw sockets that can be split into separate `send` and `receive` halves that can operate
/// independently from each other (i.e., a full-duplex connection)
pub trait RawSplit: ErrorType {
    type Receive<'a>: RawReceive<Error = Self::Error> + Readable<Error = Self::Error>
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
/// construct multiple connections that implement the raw socket traits
pub trait RawBind {
    /// Error type returned on socket creation failure
    type Error: embedded_io_async::Error;

    /// The socket type returned by the stack
    type Socket<'a>: RawReceive<Error = Self::Error>
        + RawSend<Error = Self::Error>
        + RawSplit<Error = Self::Error>
        + Readable<Error = Self::Error>
    where
        Self: 'a;

    /// Create a raw socket
    ///
    /// On most operating systems, creating a raw socket requires admin privileges.
    async fn bind(&self) -> Result<Self::Socket<'_>, Self::Error>;
}

impl<T> RawBind for &T
where
    T: RawBind,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn bind(&self) -> Result<Self::Socket<'_>, Self::Error> {
        (*self).bind().await
    }
}

impl<T> RawBind for &mut T
where
    T: RawBind,
{
    type Error = T::Error;

    type Socket<'a> = T::Socket<'a> where Self: 'a;

    async fn bind(&self) -> Result<Self::Socket<'_>, Self::Error> {
        (**self).bind().await
    }
}
