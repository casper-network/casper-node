//! Types which are serializable to JSON, which map to types defined outside this module.

mod account;
mod auction_state;
mod contracts;
mod stored_value;

use casper_types::{contracts::NamedKeys, NamedKey};

pub use account::Account;
pub use auction_state::AuctionState;
pub use contracts::{Contract, ContractPackage};
pub use stored_value::StoredValue;

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
