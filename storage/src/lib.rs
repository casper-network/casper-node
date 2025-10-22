//! Storage for a node on the Casper network.

#![doc(html_root_url = "https://docs.rs/casper-storage/3.0.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png"
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![warn(missing_docs)]

/// Address generator logic.
pub mod address_generator;
/// Block store logic.
pub mod block_store;
/// Data access layer logic.
pub mod data_access_layer;
/// Global state logic.
pub mod global_state;
/// Storage layer logic.
pub mod system;
/// Tracking copy.
pub mod tracking_copy;

pub use address_generator::{AddressGenerator, AddressGeneratorBuilder};
pub use data_access_layer::KeyPrefix;
#[cfg(test)]
pub use tracking_copy::new_temporary_tracking_copy;
pub use tracking_copy::TrackingCopy;

pub use block_store::{
    lmdb::{DbTableId, UnknownDbTableId},
    DbRawBytesSpec,
};

pub use system::runtime_native::Config as RuntimeNativeConfig;
const MESSAGING_PACKAGE_ADDR_TOPIC: &str = "package_key";
const MESSAGING_CONTRACT_ADDR_TOPIC: &str = "contract_key";
const MESSAGING_CONTRACT_BYTECODE_ADDR_TOPIC: &str = "bytecode_key";
const MESSAGING_CONTRACT_VERSION_TOPIC: &str = "contract_version";
