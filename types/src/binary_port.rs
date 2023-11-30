//! The binary port.
pub mod binary_request;
pub mod binary_response;
pub mod db_id;
pub mod get;
pub mod get_all_values;
pub mod global_state;
pub mod non_persistent_data;
pub mod speculative_execution;

const PROTOCOL_VERSION: u8 = 0;

// TODO[RC]: Move to a separate file, add bytesrepr, etc.
#[derive(Debug)]
pub struct DbRawBytesSpec {
    is_legacy: bool,
    raw_bytes: Vec<u8>,
}

impl DbRawBytesSpec {
    pub fn new_legacy(raw_bytes: &[u8]) -> Self {
        Self {
            is_legacy: true,
            raw_bytes: raw_bytes.to_vec(),
        }
    }

    pub fn new_current(raw_bytes: &[u8]) -> Self {
        Self {
            is_legacy: false,
            raw_bytes: raw_bytes.to_vec(),
        }
    }
}
