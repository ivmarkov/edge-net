use core::ops::{Deref, DerefMut};

use embassy_sync::{
    blocking_mutex::raw::RawMutex,
    mutex::{Mutex, MutexGuard},
};

/// A trait for getting access to a `&mut T` buffer, potentially awaiting until a buffer becomes available.
pub trait BufferAccess<T>
where
    T: ?Sized,
{
    type Buffer<'a>: DerefMut<Target = T>
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

impl<B, T> BufferAccess<T> for &B
where
    B: BufferAccess<T>,
    T: ?Sized,
{
    type Buffer<'a>
        = B::Buffer<'a>
    where
        Self: 'a;

    async fn get(&self) -> Option<Self::Buffer<'_>> {
        (*self).get().await
    }
}

pub struct VecBufAccess<M, const N: usize>(Mutex<M, heapless::Vec<u8, N>>)
where
    M: RawMutex;

impl<M, const N: usize> VecBufAccess<M, N>
where
    M: RawMutex,
{
    pub const fn new() -> Self {
        Self(Mutex::new(heapless::Vec::new()))
    }
}

pub struct VecBuf<'a, M, const N: usize>(MutexGuard<'a, M, heapless::Vec<u8, N>>)
where
    M: RawMutex;

impl<M, const N: usize> Drop for VecBuf<'_, M, N>
where
    M: RawMutex,
{
    fn drop(&mut self) {
        self.0.clear();
    }
}

impl<M, const N: usize> Deref for VecBuf<'_, M, N>
where
    M: RawMutex,
{
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M, const N: usize> DerefMut for VecBuf<'_, M, N>
where
    M: RawMutex,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<M, const N: usize> BufferAccess<[u8]> for VecBufAccess<M, N>
where
    M: RawMutex,
{
    type Buffer<'a>
        = VecBuf<'a, M, N>
    where
        Self: 'a;

    async fn get(&self) -> Option<Self::Buffer<'_>> {
        let mut guard = self.0.lock().await;

        unwrap!(guard.resize_default(N));

        Some(VecBuf(guard))
    }
}

impl<M, const N: usize> Default for VecBufAccess<M, N>
where
    M: RawMutex,
{
    fn default() -> Self {
        Self::new()
    }
}
