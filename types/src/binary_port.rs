//! The binary port.
mod binary_request;
mod binary_response;
mod binary_response_and_request;
mod binary_response_header;
mod error_code;
mod get_all_values_result;
mod get_request;
mod global_state_query_result;
mod information_request;
mod minimal_block_info;
#[cfg(any(feature = "std", test))]
mod node_status;
mod payload_type;
mod record_id;
mod state_request;
mod type_wrappers;

pub use binary_request::{BinaryRequest, BinaryRequestHeader, BinaryRequestTag};
pub use binary_response::BinaryResponse;
pub use binary_response_and_request::BinaryResponseAndRequest;
pub use binary_response_header::BinaryResponseHeader;
pub use error_code::ErrorCode;
pub use get_all_values_result::GetAllValuesResult;
pub use get_request::GetRequest;
pub use global_state_query_result::GlobalStateQueryResult;
pub use information_request::{InformationRequest, InformationRequestTag};
#[cfg(any(feature = "std", test))]
pub use minimal_block_info::MinimalBlockInfo;
#[cfg(any(feature = "std", test))]
pub use node_status::NodeStatus;
pub use payload_type::{PayloadEntity, PayloadType};
pub use record_id::RecordId;
pub use state_request::GlobalStateRequest;
pub use type_wrappers::{
    ConsensusStatus, ConsensusValidatorChanges, GetTrieFullResult, LastProgress, NetworkName,
    SpeculativeExecutionResult, TransactionWithExecutionInfo, Uptime,
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
