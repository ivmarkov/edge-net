#![no_std]
#![allow(async_fn_in_trait)]
#![warn(clippy::large_futures)]

use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;
use core::net::SocketAddr;
use core::ptr::NonNull;

use embassy_net::IpEndpoint;

pub use dns::*;
pub use tcp::*;
pub use udp::*;

mod dns;
mod tcp;
mod udp;

pub(crate) struct Pool<T, const N: usize> {
    used: [Cell<bool>; N],
    data: [UnsafeCell<MaybeUninit<T>>; N],
}

impl<T, const N: usize> Pool<T, N> {
    #[allow(clippy::declare_interior_mutable_const)]
    const VALUE: Cell<bool> = Cell::new(false);
    #[allow(clippy::declare_interior_mutable_const)]
    const UNINIT: UnsafeCell<MaybeUninit<T>> = UnsafeCell::new(MaybeUninit::uninit());

    const fn new() -> Self {
        Self {
            used: [Self::VALUE; N],
            data: [Self::UNINIT; N],
        }
    }
}

impl<T, const N: usize> Pool<T, N> {
    fn alloc(&self) -> Option<NonNull<T>> {
        for n in 0..N {
            // this can't race because Pool is not Sync.
            if !self.used[n].get() {
                self.used[n].set(true);
                let p = self.data[n].get() as *mut T;
                return Some(unsafe { NonNull::new_unchecked(p) });
            }
        }
        None
    }

    /// safety: p must be a pointer obtained from self.alloc that hasn't been freed yet.
    unsafe fn free(&self, p: NonNull<T>) {
        let origin = self.data.as_ptr() as *mut T;
        let n = p.as_ptr().offset_from(origin);
        assert!(n >= 0);
        assert!((n as usize) < N);
        self.used[n as usize].set(false);
    }
}

pub(crate) fn to_net_socket(socket: IpEndpoint) -> SocketAddr {
    SocketAddr::new(socket.addr.into(), socket.port)
}

// pub(crate) fn to_net_socket2(socket: IpListenEndpoint) -> SocketAddr {
//     SocketAddr::new(
//         socket
//             .addr
//             .map(to_net_addr)
//             .unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
//         socket.port,
//     )
// }
