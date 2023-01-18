pub mod http;
pub mod io;
#[cfg(all(feature = "std", feature = "rumqttc"))]
pub mod rumqttc;
#[cfg(feature = "std")]
pub mod stdnal;
pub mod tcp;
pub mod ws;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc::executor::asynch::Unblocker;

#[cfg(not(feature = "embedded-svc"))]
pub use unblocker::Unblocker;

#[cfg(not(feature = "embedded-svc"))]
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
