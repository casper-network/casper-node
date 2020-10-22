//! Types which are serializable to JSON, and which map to types defined outside this crate.

use std::collections::BTreeMap;

use casper_types::Key;

mod account;
mod auction_state;
mod cl_value;
mod deploy_info;
mod execution_result;
mod public_key;
mod stored_value;
mod transfer;

pub use account::Account;
pub use auction_state::{AuctionState, Bid, Bids};
pub use cl_value::CLValue;
pub use deploy_info::DeployInfo;
pub use execution_result::ExecutionResult;
pub use public_key::PublicKey;
pub use stored_value::StoredValue;
pub use transfer::Transfer;

fn convert_named_keys(named_keys: &BTreeMap<String, Key>) -> BTreeMap<String, String> {
    named_keys
        .iter()
        .map(|(name, key)| (name.clone(), key.to_formatted_string()))
        .collect()
}
