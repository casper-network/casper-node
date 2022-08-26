use std::{borrow::Cow, convert::Infallible};

use casper_execution_engine::storage::trie::TrieRaw;
use casper_types::bytesrepr::Bytes;

/// Implemented for types that are chunked when sending over the wire and/or before storing the the
/// trie store.
pub trait Chunkable {
    /// Error returned when mapping `Self` into bytes.
    type Error: std::fmt::Debug;

    /// Maps `Self` into bytes.
    ///
    /// Returnes a [`Cow`] instance in case the resulting bytes are the same as input and we don't
    /// want to reinitialize. This also helps with a case where returning a vector of bytes
    /// would require instantiating a `Vec<u8>` locally (see [`ToBytes::to_bytes`]) but can't be
    /// returned as reference. Alternative encoding would be to consume `Self` and return `Vec<u8>`
    /// but that may do it unnecessarily if `Self` would be to used again.
    fn as_bytes(&self) -> Result<Cow<Vec<u8>>, Self::Error>;
}

impl Chunkable for Vec<u8> {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<Cow<Vec<u8>>, Self::Error> {
        Ok(Cow::Borrowed(self))
    }
}

impl Chunkable for Bytes {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<Cow<Vec<u8>>, Self::Error> {
        Ok(Cow::Borrowed(self.inner_bytes()))
    }
}

impl Chunkable for TrieRaw {
    type Error = Infallible;

    fn as_bytes(&self) -> Result<Cow<Vec<u8>>, Self::Error> {
        Ok(Cow::Borrowed(self.inner().inner_bytes()))
    }
}
