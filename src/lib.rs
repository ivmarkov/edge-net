#![cfg_attr(not(feature = "std"), no_std)]
#![feature(cfg_version)]
#![feature(generic_associated_types)] // For mutex, http, http::client, http::server, ota, ghota and all asynch; soon to be stabilized
#![feature(cfg_target_has_atomic)] // Soon to be stabilized
#![cfg_attr(feature = "experimental", feature(type_alias_impl_trait))] // For the Sender/Receiver adapters; hopefully soon to be stabilized
#![cfg_attr(version("1.61"), allow(deprecated_where_clause_location))]
//#![feature(type_alias_impl_trait)]

#[cfg(feature = "alloc")]
#[allow(unused_imports)]
#[macro_use]
extern crate alloc;

pub mod asynch;
