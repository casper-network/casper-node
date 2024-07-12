//! The result of the query for the global state value.

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    global_state::TrieMerkleProof,
    Key, StoredValue,
};

#[cfg(test)]
use casper_types::testing::TestRng;

#[cfg(test)]
use casper_types::{ByteCode, ByteCodeKind};
use serde::Serialize;

/// Carries the successful result of the global state query.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct GlobalStateQueryResult {
    /// Stored value.
    value: StoredValue,
    /// Proof.
    merkle_proof: Vec<TrieMerkleProof<Key, StoredValue>>,
}

impl GlobalStateQueryResult {
    /// Creates the global state query result.
    pub fn new(value: StoredValue, merkle_proof: Vec<TrieMerkleProof<Key, StoredValue>>) -> Self {
        Self {
            value,
            merkle_proof,
        }
    }

    /// Returns the stored value.
    pub fn value(&self) -> &StoredValue {
        &self.value
    }

    /// Returns the stored value and the merkle proof.
    pub fn into_inner(self) -> (StoredValue, Vec<TrieMerkleProof<Key, StoredValue>>) {
        (self.value, self.merkle_proof)
    }

    #[cfg(test)]
    pub(crate) fn random_invalid(rng: &mut TestRng) -> Self {
        use casper_types::{global_state::TrieMerkleProofStep, CLValue};
        use rand::Rng;
        // Note: This does NOT create a logically-valid struct. Instance created by this function
        // should be used in `bytesrepr` tests only.

        let mut merkle_proof = vec![];
        for _ in 0..rng.gen_range(0..10) {
            let stored_value = StoredValue::CLValue(
                CLValue::from_t(rng.gen::<i32>()).expect("should create CLValue"),
            );
            let steps = (0..rng.gen_range(0..10))
                .map(|_| TrieMerkleProofStep::random(rng))
                .collect();
            merkle_proof.push(TrieMerkleProof::new(rng.gen(), stored_value, steps));
        }

        Self {
            value: StoredValue::ByteCode(ByteCode::new(
                ByteCodeKind::V1CasperWasm,
                rng.random_vec(10..20),
            )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = GlobalStateQueryResult::random_invalid(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
