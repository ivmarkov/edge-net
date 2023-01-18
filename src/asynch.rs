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
        type UnblockFuture<T>: Future<Output = T> + Send
        where
            T: Send;

        fn unblock<F, T>(&self, f: F) -> Self::UnblockFuture<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static;
    }

    impl<U> Unblocker for &U
    where
        U: Unblocker,
    {
        type UnblockFuture<T>
        = U::UnblockFuture<T> where T: Send;

        fn unblock<F, T>(&self, f: F) -> Self::UnblockFuture<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            (*self).unblock(f)
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
        U: embedded_svc::executor::asynch::Unblocker,
    {
        type UnblockFuture<T> = impl Future<Output = T> + Send
        where T: Send;

        fn unblock<F, T>(&self, f: F) -> Self::UnblockFuture<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            self.0.unblock(f)
        }
    }

    impl<U> embedded_svc::executor::asynch::Unblocker for UnblockerCompat<U>
    where
        U: Unblocker,
    {
        type UnblockFuture<T> = impl Future<Output = T> + Send
        where T: Send;

        fn unblock<F, T>(&self, f: F) -> Self::UnblockFuture<T>
        where
            F: FnOnce() -> T + Send + 'static,
            T: Send + 'static,
        {
            self.0.unblock(f)
        }
    }
}
