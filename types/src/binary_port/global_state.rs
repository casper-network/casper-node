//! The result of the query for the global state value.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    StoredValue,
};
use alloc::string::String;
use alloc::vec::Vec;

/// Carries the successful result of the global state query.
pub struct GlobalStateQueryResult {
    /// Stored value.
    value: StoredValue,
    /// Proof.
    merkle_proof: String,
}

impl GlobalStateQueryResult {
    pub fn new(value: StoredValue, merkle_proof: String) -> Self {
        Self {
            value,
            merkle_proof,
        }
    }
}

impl ToBytes for GlobalStateQueryResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let GlobalStateQueryResult {
            value,
            merkle_proof,
        } = self;
        value.write_bytes(writer)?;
        merkle_proof.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.value.serialized_length() + self.merkle_proof.serialized_length()
    }
}

impl FromBytes for GlobalStateQueryResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, remainder) = FromBytes::from_bytes(bytes)?;
        let (merkle_proof, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            GlobalStateQueryResult {
                value,
                merkle_proof,
            },
            remainder,
        ))
    }
}
