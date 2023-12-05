#[cfg(feature = "nightly")]
pub mod nal;

use embassy_sync::blocking_mutex::raw::RawMutex;

pub struct StdRawMutex(std::sync::Mutex<()>);

unsafe impl RawMutex for StdRawMutex {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self(std::sync::Mutex::new(()));

    fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.lock().unwrap();

        f()
    }
}
