#![cfg_attr(not(feature = "std"), no_std)]
#![feature(cfg_version)]
#![cfg_attr(feature = "nightly", feature(type_alias_impl_trait))]
#![cfg_attr(
    all(feature = "nightly", version("1.70")),
    feature(impl_trait_in_assoc_type)
)]
#![cfg_attr(
    feature = "nightly",
    feature(async_fn_in_trait),
    feature(impl_trait_projections),
    allow(incomplete_features)
)]

#[cfg(feature = "nightly")]
pub mod asynch;
#[cfg(feature = "domain")]
pub mod captive;
#[cfg(feature = "std")]
pub mod std_mutex;
