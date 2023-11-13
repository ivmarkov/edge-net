pub mod dhcp;
pub mod http;
pub mod io;
#[cfg(all(feature = "std", feature = "rumqttc"))]
pub mod rumqttc;
#[cfg(feature = "std")]
pub mod stdnal;
pub mod tcp;
pub mod ws;

pub use unblocker::Unblocker;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

mod unblocker {
    use core::future::Future;

    pub trait Unblocker {
        type UnblockFuture<'a, F, T>: Future<Output = T> + Send
        where
            Self: 'a,
            F: Send + 'a,
            T: Send + 'a;

        fn unblock<'a, F, T>(&'a self, f: F) -> Self::UnblockFuture<'a, F, T>
        where
            F: FnOnce() -> T + Send + 'a,
            T: Send + 'a;
    }

    impl<U> Unblocker for &U
    where
        U: Unblocker,
    {
        type UnblockFuture<'a, F, T>
        = U::UnblockFuture<'a, F, T> where Self: 'a, F: Send + 'a, T: Send + 'a;

        fn unblock<'a, F, T>(&'a self, f: F) -> Self::UnblockFuture<'a, F, T>
        where
            F: FnOnce() -> T + Send + 'a,
            T: Send + 'a,
        {
            (*self).unblock(f)
        }
    }

    impl<U> Unblocker for &mut U
    where
        U: Unblocker,
    {
        type UnblockFuture<'a, F, T>
        = U::UnblockFuture<'a, F, T> where Self: 'a, F: Send + 'a, T: Send + 'a;

        fn unblock<'a, F, T>(&'a self, f: F) -> Self::UnblockFuture<'a, F, T>
        where
            F: FnOnce() -> T + Send + 'a,
            T: Send + 'a,
        {
            (**self).unblock(f)
        }
    }
}

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;

    use super::Unblocker;

    pub struct UnblockerCompat<U>(U);

    impl<U> Unblocker for UnblockerCompat<U>
    where
        U: embedded_svc::utils::asyncify::Unblocker,
    {
        type UnblockFuture<'a, F, T> = impl Future<Output = T> + Send
        where Self: 'a, F: Send + 'a, T: Send + 'a;

        fn unblock<'a, F, T>(&'a self, f: F) -> Self::UnblockFuture<'a, F, T>
        where
            F: FnOnce() -> T + Send + 'a,
            T: Send + 'a,
        {
            self.0.unblock(f)
        }
    }

    impl<U> embedded_svc::utils::asyncify::Unblocker for UnblockerCompat<U>
    where
        U: Unblocker,
    {
        type UnblockFuture<'a, F, T> = impl Future<Output = T> + Send
        where Self: 'a, F: Send + 'a, T: Send + 'a;

        fn unblock<'a, F, T>(&'a self, f: F) -> Self::UnblockFuture<'a, F, T>
        where
            F: FnOnce() -> T + Send + 'a,
            T: Send + 'a,
        {
            self.0.unblock(f)
        }
    }
}
