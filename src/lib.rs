#![cfg_attr(not(feature = "std"), no_std)]
#![allow(stable_features)]
#![allow(unknown_lints)]
#![cfg_attr(feature = "nightly", feature(async_fn_in_trait))]
#![cfg_attr(feature = "nightly", allow(async_fn_in_trait))]
#![cfg_attr(feature = "nightly", feature(impl_trait_projections))]

pub use edge_captive as captive;
pub use edge_dhcp as dhcp;
pub use edge_http as http;
pub use edge_mdns as mdns;
pub use edge_mqtt as mqtt;
pub use edge_raw as raw;
pub use edge_std_nal_async as std_nal;
pub use edge_ws as ws;
