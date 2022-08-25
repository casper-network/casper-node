use std::convert::Infallible;

use casper_execution_engine::{core::engine_state::ExecutionResult, storage::trie::TrieRaw};
use casper_types::bytesrepr::Bytes;

/// Implemented for types that are chunked when sending over the wire and/or before storing the the
/// trie store.
pub trait Chunkable {
    /// Error returned when mapping `Self` into bytes.
    type Error: std::fmt::Debug;

    /// Maps `Self` into bytes.
    fn as_bytes(&self) -> Result<&[u8], Self::Error>;
}

impl Chunkable for Vec<u8> {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<&[u8], Self::Error> {
        Ok(self.as_slice())
    }
}

impl Chunkable for Bytes {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<&[u8], Self::Error> {
        Ok(self.inner_bytes().as_slice())
    }
}

impl Chunkable for &[u8] {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<&[u8], Self::Error> {
        Ok(self)
    }
}

impl Chunkable for [u8] {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<&[u8], Self::Error> {
        Ok(self)
    }
}

impl Chunkable for TrieRaw {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<&[u8], Self::Error> {
        Ok(self.inner().inner_bytes())
    }
}
