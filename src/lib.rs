#![cfg_attr(not(feature = "std"), no_std)]
#![allow(stable_features)]
#![allow(unknown_lints)]
#![cfg_attr(feature = "nightly", feature(async_fn_in_trait))]
#![cfg_attr(feature = "nightly", allow(async_fn_in_trait))]
#![cfg_attr(feature = "nightly", feature(impl_trait_projections))]
#![cfg_attr(feature = "nightly", feature(impl_trait_in_assoc_type))]

#[cfg(feature = "nightly")]
pub mod asynch;
#[cfg(feature = "domain")]
pub mod captive;
#[cfg(feature = "domain")]
pub mod mdns;
#[cfg(feature = "std")]
pub mod std_mutex;
