#![cfg_attr(not(feature = "std"), no_std)]
#![allow(stable_features)]
#![allow(unknown_lints)]
#![allow(async_fn_in_trait)]
#![cfg_attr(feature = "nightly", feature(async_fn_in_trait))]
#![cfg_attr(feature = "nightly", feature(impl_trait_projections))]

#[cfg(feature = "nightly")]
pub use stack::*;

#[cfg(feature = "nightly")]
mod stack;
