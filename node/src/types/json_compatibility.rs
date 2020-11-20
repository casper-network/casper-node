//! Types which are serializable to JSON, which map to types defined outside this module.

mod auction_state;
mod deploy_info;
mod execution_result;
mod stored_value;

pub use auction_state::AuctionState;
pub use deploy_info::DeployInfo;
pub use execution_result::ExecutionResult;
pub use stored_value::StoredValue;
