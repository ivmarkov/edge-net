#![cfg_attr(not(feature = "std"), no_std)]
#![allow(async_fn_in_trait)]

pub use edge_captive as captive;
pub use edge_dhcp as dhcp;
pub use edge_http as http;
pub use edge_mdns as mdns;
#[cfg(feature = "std")]
pub use edge_mqtt as mqtt;
pub use edge_raw as raw;
#[cfg(feature = "std")]
pub use edge_std_nal_async as std_nal;
pub use edge_ws as ws;
#[cfg(feature = "io")]
pub use embedded_nal_async_xtra;
