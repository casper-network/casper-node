//! Types which are serializable to JSON, and which map to types defined outside this crate.

use std::collections::BTreeMap;

use casper_types::Key;

mod account;
mod execution_result;
mod stored_value;

pub use account::Account;
pub use execution_result::ExecutionResult;
pub use stored_value::StoredValue;

fn convert_named_keys(named_keys: &BTreeMap<String, Key>) -> BTreeMap<String, String> {
    named_keys
        .iter()
        .map(|(name, key)| (name.clone(), key.to_formatted_string()))
        .collect()
}
