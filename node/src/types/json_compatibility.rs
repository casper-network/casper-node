//! Types which are serializable to JSON, and which map to types defined outside this crate.

mod auction_state;
mod deploy_info;
mod execution_result;
mod key_value_pair;
mod stored_value;

pub use auction_state::AuctionState;
pub use deploy_info::DeployInfo;
pub use execution_result::ExecutionResult;
pub use key_value_pair::KeyValuePair;
pub use stored_value::StoredValue;

/*
fn convert_named_keys(named_keys: &BTreeMap<String, Key>) -> BTreeMap<String, String> {
    named_keys
        .iter()
        .map(|(name, key)| (name.clone(), key.to_formatted_string()))
        .collect()
}
*/
