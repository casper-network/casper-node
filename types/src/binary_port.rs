//! The binary port.
mod binary_request;
mod binary_response;
mod binary_response_and_request;
mod binary_response_header;
mod db_id;
mod error_code;
mod get;
mod get_all_values_result;
mod global_state_query_result;
mod minimal_block_info;
#[cfg(any(feature = "std", test))]
mod node_status;
mod non_persistent_data_request;
mod payload_type;
mod type_wrappers;

pub use binary_request::{BinaryRequest, BinaryRequestHeader, BinaryRequestTag};
pub use binary_response::BinaryResponse;
pub use binary_response_and_request::BinaryResponseAndRequest;
pub use binary_response_header::BinaryResponseHeader;
pub use db_id::DbId;
pub use error_code::ErrorCode;
pub use get::GetRequest;
pub use get_all_values_result::GetAllValuesResult;
pub use global_state_query_result::GlobalStateQueryResult;
#[cfg(any(feature = "std", test))]
pub use minimal_block_info::MinimalBlockInfo;
#[cfg(any(feature = "std", test))]
pub use node_status::NodeStatus;
pub use non_persistent_data_request::NonPersistedDataRequest;
pub use payload_type::{PayloadEntity, PayloadType};
pub use type_wrappers::{
    ConsensusStatus, ConsensusValidatorChanges, GetTrieFullResult, HighestBlockSequenceCheckResult,
    LastProgress, NetworkName, SpeculativeExecutionResult, Uptime,
};

use alloc::vec::Vec;

/// Stores raw bytes from the DB along with the flag indicating whether data come from legacy or
/// current version of the DB.
#[derive(Debug)]
pub struct DbRawBytesSpec {
    is_legacy: bool,
    raw_bytes: Vec<u8>,
}

impl DbRawBytesSpec {
    /// Creates a variant indicating that raw bytes are coming from the legacy database.
    pub fn new_legacy(raw_bytes: &[u8]) -> Self {
        Self {
            is_legacy: true,
            raw_bytes: raw_bytes.to_vec(),
        }
    }

    /// Creates a variant indicating that raw bytes are coming from the current database.
    pub fn new_current(raw_bytes: &[u8]) -> Self {
        Self {
            is_legacy: false,
            raw_bytes: raw_bytes.to_vec(),
        }
    }
}
