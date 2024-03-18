mod block_provider;
mod error;
pub mod lmdb;
pub mod record_id;
pub mod types;

pub use block_provider::{BlockStoreProvider, BlockStoreTransaction, DataReader, DataWriter};
pub use error::BlockStoreError;
pub use record_id::{RecordId, UnknownRecordId};

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

    /// Is legacy?
    pub fn is_legacy(&self) -> bool {
        self.is_legacy
    }

    /// Raw bytes.
    pub fn raw_bytes(&self) -> Vec<u8> {
        self.raw_bytes.clone()
    }
}
