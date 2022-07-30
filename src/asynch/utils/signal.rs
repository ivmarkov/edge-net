/// This module is an adaptation of Embassy's signal (https://github.com/embassy-rs/embassy/blob/master/embassy/src/channel/signal.rs)
/// with a generified Mutex where Embassy originally utilizes a critical section.
use core::mem;
use core::task::{Context, Poll, Waker};

use crate::mutex::{RawMutex, StdRawMutex};
use crate::signal::asynch::Signal;
use crate::utils::mutex::Mutex;

#[cfg(target_has_atomic = "ptr")]
pub use atomic_signal::*;

/// Synchronization primitive. Allows creating awaitable signals that may be passed between tasks.
/// For a simple use-case where the receiver is only ever interested in the latest value of
/// something, Signals work well.
pub struct MutexSignal<R, T>(Mutex<R, State<T>>);

enum State<T> {
    None,
    Waiting(Waker),
    Signaled(T),
}

impl<R, T> MutexSignal<R, T>
where
    R: RawMutex,
{
    pub fn new() -> Self {
        Self(Mutex::new(State::None))
    }

    pub fn signaled(&self) -> bool {
        let state = self.0.lock();

        matches!(&*state, State::Signaled(_))
    }

    fn new() -> Self {
        Default::default()
    }

    fn reset(&self) {
        let mut state = self.0.lock();

        *state = State::None
    }

    fn signal(&self, data: T) {
        let mut state = self.0.lock();

        if let State::Waiting(waker) = mem::replace(&mut *state, State::Signaled(data)) {
            waker.wake();
        }
    }

    fn poll_wait(&self, cx: &mut Context<'_>) -> Poll<T> {
        let mut state = self.0.lock();

        match &mut *state {
            State::None => {
                *state = State::Waiting(cx.waker().clone());
                Poll::Pending
            }
            State::Waiting(w) if w.will_wake(cx.waker()) => Poll::Pending,
            State::Waiting(_) => panic!("waker overflow"),
            State::Signaled(_) => match mem::replace(&mut *state, State::None) {
                State::Signaled(data) => Poll::Ready(data),
                _ => unreachable!(),
            },
        }
    }

    fn is_set(&self) -> bool {
        let state = self.0.lock();

        matches!(&*state, State::Signaled(_))
    }

    fn try_get(&self) -> Option<Self::Data> {
        let mut state = self.0.lock();

        match &mut *state {
            State::Signaled(_) => match mem::replace(&mut *state, State::None) {
                State::Signaled(res) => Some(res),
                _ => unreachable!(),
            },
            _ => None,
        }
    }
}

impl<R, T> Default for MutexSignal<R, T>
where
    R: RawMutex,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_has_atomic = "ptr")]
mod atomic_signal {
    use core::marker::PhantomData;
    use core::mem;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use core::task::{Context, Poll};

    use futures::task::AtomicWaker;

    use crate::signal::asynch::Signal;

    pub struct AtomicSignal<T> {
        waker: AtomicWaker,
        data: AtomicUsize,
        _type: PhantomData<Option<T>>,
    }

    impl<T> AtomicSignal<T>
    where
        T: Copy,
    {
        pub fn new() -> Self {
            if mem::size_of::<Option<T>>() > mem::size_of::<usize>() {
                panic!("Cannot fit the value in usize");
            }

            Self {
                data: AtomicUsize::new(Self::to_usize(None)),
                waker: AtomicWaker::new(),
                _type: PhantomData,
            }
        }

        fn new() -> Self {
            Default::default()
        }

        fn reset(&self) {
            self.data.store(Self::to_usize(None), Ordering::SeqCst);
            self.waker.take();
        }

        fn signal(&self, data: T) {
            self.data
                .store(Self::to_usize(Some(data)), Ordering::SeqCst);
            self.waker.wake();
        }

        fn poll_wait(&self, cx: &mut Context<'_>) -> Poll<T> {
            self.waker.register(cx.waker());

            if let Some(data) =
                Self::from_usize(self.data.swap(Self::to_usize(None), Ordering::SeqCst))
            {
                Poll::Ready(data)
            } else {
                Poll::Pending
            }
        }

        fn is_set(&self) -> bool {
            Self::from_usize(self.data.load(Ordering::SeqCst)).is_some()
        }

        fn try_get(&self) -> Option<Self::Data> {
            let data = Self::from_usize(self.data.swap(Self::to_usize(None), Ordering::SeqCst));
            self.waker.take();

            data
        }

        fn to_usize(data: Option<T>) -> usize {
            let src_arr: &[u8; mem::size_of::<usize>()] = unsafe { mem::transmute(&data) };
            let mut dst_arr = [0_u8; mem::size_of::<usize>()];

            dst_arr[0..mem::size_of::<Option<T>>()]
                .copy_from_slice(&src_arr[0..mem::size_of::<Option<T>>()]);
            usize::from_ne_bytes(dst_arr)
        }

        fn from_usize(value: usize) -> Option<T> {
            let src_arr = usize::to_ne_bytes(value);
            let data: &Option<T> = unsafe { mem::transmute(&src_arr) };

            *data
        }
    }

    impl<T> Default for AtomicSignal<T>
    where
        T: Copy,
    {
        fn default() -> Self {
            Self::new()
        }
    }
}
