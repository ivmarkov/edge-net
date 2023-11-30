#![cfg_attr(not(feature = "std"), no_std)]
#![allow(stable_features)]
#![cfg_attr(feature = "nightly", feature(impl_trait_in_assoc_type))] // Used in Unblocker

// Re-export enabled sub-crates
#[cfg(feature = "edge-captive")]
pub use edge_captive as captive;
#[cfg(feature = "edge-dhcp")]
pub use edge_dhcp as dhcp;
#[cfg(feature = "edge-http")]
pub use edge_http as http;
#[cfg(feature = "edge-mdns")]
pub use edge_mdns as mdns;
#[cfg(feature = "edge-mqtt")]
pub use edge_mqtt as mqtt;
#[cfg(feature = "edge-ws")]
pub use edge_ws as ws;

#[cfg(feature = "nightly")]
pub mod asynch;

#[cfg(feature = "std")]
pub mod std;
