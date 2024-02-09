//! Storage for a node on the Casper network.

#![doc(html_root_url = "https://docs.rs/casper-storage/1.4.3")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod address_generator;
pub mod data_access_layer;
pub mod global_state;
pub mod system;
pub mod tracking_copy;

pub use address_generator::{AddressGenerator, AddressGeneratorBuilder};
pub use tracking_copy::TrackingCopy;

#[cfg(test)]
pub use self::tracking_copy::new_temporary_tracking_copy;
