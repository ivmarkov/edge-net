#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "nightly", feature(generic_associated_types))]
#![cfg_attr(feature = "nightly", feature(type_alias_impl_trait))]

#[cfg(feature = "nightly")]
pub mod asynch;
#[cfg(feature = "domain")]
pub mod captive;
#[cfg(all(feature = "std", feature = "embassy-util"))]
pub mod std_mutex;
pub mod utils;
