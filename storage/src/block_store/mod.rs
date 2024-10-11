mod block_provider;
mod error;
/// Block store lmdb logic.
pub mod lmdb;
/// Block store types.
pub mod types;

pub use block_provider::{BlockStoreProvider, BlockStoreTransaction, DataReader, DataWriter};
pub use error::BlockStoreError;

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
    pub fn into_raw_bytes(self) -> Vec<u8> {
        self.raw_bytes
    }
}
