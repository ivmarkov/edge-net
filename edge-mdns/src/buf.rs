use core::ops::DerefMut;

use embassy_sync::{
    blocking_mutex::raw::RawMutex,
    mutex::{Mutex, MutexGuard},
};

/// A trait for getting access to a buffer, potentially awaiting until a buffer becomes available.
pub trait BufferAccess {
    type BufferSurface;

    type Buffer<'a>: DerefMut<Target = Self::BufferSurface>
    where
        Self: 'a;

    /// Get a reference to a buffer.
    /// Might await until a buffer is available, as it might be in use by somebody else.
    ///
    /// Depending on its internal implementation details, access to a buffer might also be denied
    /// immediately, or after a certain amount of time (subject to the concrete implementation of the method).
    /// In that case, the method will return `None`.
    async fn get(&self) -> Option<Self::Buffer<'_>>;
}

impl<B> BufferAccess for &B
where
    B: BufferAccess,
{
    type BufferSurface = B::BufferSurface;

    type Buffer<'a> = B::Buffer<'a> where Self: 'a;

    async fn get(&self) -> Option<Self::Buffer<'_>> {
        (*self).get().await
    }
}

impl<B> BufferAccess for &mut B
where
    B: BufferAccess,
{
    type BufferSurface = B::BufferSurface;

    type Buffer<'a> = B::Buffer<'a> where Self: 'a;

    async fn get(&self) -> Option<Self::Buffer<'_>> {
        (**self).get().await
    }
}

impl<M, T> BufferAccess for Mutex<M, T>
where
    M: RawMutex,
{
    type BufferSurface = T;

    type Buffer<'a> = MutexGuard<'a, M, T> where Self: 'a;

    async fn get(&self) -> Option<Self::Buffer<'_>> {
        Some(self.lock().await)
    }
}
