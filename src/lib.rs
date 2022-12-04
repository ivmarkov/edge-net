#![feature(cfg_version)]
#![cfg_attr(
    all(feature = "nightly", not(version("1.65"))),
    feature(generic_associated_types)
)]
#![cfg_attr(feature = "nightly", feature(type_alias_impl_trait))]
#![cfg_attr(all(feature = "nightly", version("1.67")), allow(incomplete_features))]
#![cfg_attr(all(feature = "nightly", version("1.67")), feature(async_fn_in_trait))]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "nightly")]
pub mod asynch;
#[cfg(feature = "domain")]
pub mod captive;
#[cfg(feature = "std")]
pub mod std_mutex;
