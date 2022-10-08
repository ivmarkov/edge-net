// TODO: Move into a micro-crate

use core::fmt::Debug;
use core::future::Future;

pub trait Sender {
    type Error: Debug;

    type Data;

    type SendFuture<'a>: Future<Output = Result<(), Self::Error>>
    where
        Self: 'a;

    fn send<'a>(&'a mut self, data: &'a Self::Data) -> Self::SendFuture<'a>;
}

impl<'t, T> Sender for &'t mut T
where
    T: Sender + 't,
{
    type Error = T::Error;

    type Data = T::Data;

    type SendFuture<'a> = impl Future<Output = Result<(), Self::Error>> where Self: 'a;

    fn send<'a>(&'a mut self, data: &'a Self::Data) -> Self::SendFuture<'a> {
        async move { (*self).send(data).await }
    }
}

pub trait Receiver {
    type Error: Debug;

    type Data;

    type RecvFuture<'a>: Future<Output = Result<Self::Data, Self::Error>>
    where
        Self: 'a;

    fn recv(&mut self) -> Self::RecvFuture<'_>;
}

impl<'t, T> Receiver for &'t mut T
where
    T: Receiver + 't,
{
    type Error = T::Error;

    type Data = T::Data;

    type RecvFuture<'a> = impl Future<Output = Result<Self::Data, Self::Error>> where Self: 'a;

    fn recv(&mut self) -> Self::RecvFuture<'_> {
        async move { (*self).recv().await }
    }
}
