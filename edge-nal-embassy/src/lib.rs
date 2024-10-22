#![no_std]
#![allow(async_fn_in_trait)]
#![warn(clippy::large_futures)]

use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;
use core::net::{IpAddr, SocketAddr};
use core::ptr::NonNull;

use embassy_net::{IpAddress, IpEndpoint, IpListenEndpoint};

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
    SocketAddr::new(to_net_addr(socket.addr), socket.port)
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

pub(crate) fn to_emb_socket(socket: SocketAddr) -> IpEndpoint {
    IpEndpoint {
        addr: to_emb_addr(socket.ip()),
        port: socket.port(),
    }
}

pub(crate) fn to_emb_bind_socket(socket: SocketAddr) -> IpListenEndpoint {
    IpListenEndpoint {
        addr: (!socket.ip().is_unspecified()).then(|| to_emb_addr(socket.ip())),
        port: socket.port(),
    }
}

pub(crate) fn to_net_addr(addr: IpAddress) -> IpAddr {
    match addr {
        //#[cfg(feature = "proto-ipv4")]
        IpAddress::Ipv4(addr) => addr.into(),
        // #[cfg(not(feature = "proto-ipv4"))]
        // IpAddr::V4(_) => panic!("ipv4 support not enabled"),
        //#[cfg(feature = "proto-ipv6")]
        IpAddress::Ipv6(addr) => addr.into(),
        // #[cfg(not(feature = "proto-ipv6"))]
        // IpAddr::V6(_) => panic!("ipv6 support not enabled"),
    }
}

pub(crate) fn to_emb_addr(addr: IpAddr) -> IpAddress {
    match addr {
        //#[cfg(feature = "proto-ipv4")]
        IpAddr::V4(addr) => IpAddress::Ipv4(addr),
        // #[cfg(not(feature = "proto-ipv4"))]
        // IpAddr::V4(_) => panic!("ipv4 support not enabled"),
        //#[cfg(feature = "proto-ipv6")]
        IpAddr::V6(addr) => IpAddress::Ipv6(addr),
        // #[cfg(not(feature = "proto-ipv6"))]
        // IpAddr::V6(_) => panic!("ipv6 support not enabled"),
    }
}
