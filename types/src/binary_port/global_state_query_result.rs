//! The result of the query for the global state value.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    StoredValue,
};
use alloc::{string::String, vec::Vec};

#[cfg(test)]
use crate::testing::TestRng;

#[cfg(test)]
use crate::{ByteCode, ByteCodeKind};

/// Carries the successful result of the global state query.
#[derive(Debug, PartialEq, Clone)]
pub struct GlobalStateQueryResult {
    /// Stored value.
    value: StoredValue,
    /// Proof.
    merkle_proof: String,
}

impl GlobalStateQueryResult {
    /// Creates the global state query result.
    pub fn new(value: StoredValue, merkle_proof: String) -> Self {
        Self {
            value,
            merkle_proof,
        }
    }

    /// Returns the stored value and the merkle proof.
    pub fn into_inner(self) -> (StoredValue, String) {
        (self.value, self.merkle_proof)
    }

    #[cfg(test)]
    pub(crate) fn random_invalid(rng: &mut TestRng) -> Self {
        // Note: This does NOT create a logically-valid struct. Instance created by this function
        // should be used in `bytesrepr` tests only.
        Self {
            value: StoredValue::ByteCode(ByteCode::new(
                ByteCodeKind::V1CasperWasm,
                rng.random_vec(10..20),
            )),
            merkle_proof: rng.random_string(10..20),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = GlobalStateQueryResult::random_invalid(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
