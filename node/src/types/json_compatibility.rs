//! Types which are serializable to JSON, which map to types defined outside this module.

mod account;
mod auction_state;
mod contracts;
mod stored_value;

use hex_buffer_serde::{Hex, HexForm};
use serde::{Deserialize, Serialize};

pub use account::Account;
pub use auction_state::AuctionState;
use casper_types::{contracts::NamedKeys, NamedKey};
pub use contracts::{Contract, ContractPackage};
pub use stored_value::StoredValue;

/// Newtype wrapper for serializing Vec<u8> as base16.
#[derive(Serialize, Deserialize, Debug)]
pub struct Base16Blob(#[serde(with = "HexForm")] Vec<u8>);

impl From<Vec<u8>> for Base16Blob {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<Base16Blob> for Vec<u8> {
    fn from(blob: Base16Blob) -> Self {
        blob.0
    }
}

/// A helper function to change NamedKeys into a Vec<NamedKey>
pub fn vectorize(keys: &NamedKeys) -> Vec<NamedKey> {
    let named_keys = keys
        .iter()
        .map(|(name, key)| NamedKey {
            name: name.clone(),
            key: key.to_formatted_string(),
        })
        .collect();
    named_keys
}
