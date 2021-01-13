//! Types which are serializable to JSON, which map to types defined outside this module.

mod account;
mod auction_state;
mod contracts;
mod stored_value;

pub use account::Account;
pub use auction_state::AuctionState;
use casper_types::{contracts::NamedKeys, NamedKey};
pub use contracts::{Contract, ContractPackage};
pub use stored_value::StoredValue;

/// A helper function to extract NamedKeys into a JSON compatible Vec<NamedKey>
pub fn vectorize(keys: &NamedKeys) -> Vec<NamedKey> {
    keys.iter()
        .map(|(name, key)| NamedKey {
            name: name.clone(),
            key: key.to_formatted_string(),
        })
        .collect()
}
