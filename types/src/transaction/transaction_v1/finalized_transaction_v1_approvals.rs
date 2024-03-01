use alloc::{collections::BTreeSet, vec::Vec};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    TransactionV1Approval,
};

/// A set of approvals that has been agreed upon by consensus to approve of a specific
/// `TransactionV1`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct FinalizedTransactionV1Approvals(BTreeSet<TransactionV1Approval>);

impl FinalizedTransactionV1Approvals {
    /// Creates a new set of finalized transaction approvals.
    pub fn new(approvals: BTreeSet<TransactionV1Approval>) -> Self {
        Self(approvals)
    }

    /// Returns the inner `BTreeSet` of approvals.
    pub fn inner(&self) -> &BTreeSet<TransactionV1Approval> {
        &self.0
    }

    /// Converts this set of finalized approvals into the inner `BTreeSet`.
    pub fn into_inner(self) -> BTreeSet<TransactionV1Approval> {
        self.0
    }

    /// Returns a random FinalizedTransactionV1Approvals.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let count = rng.gen_range(1..10);
        let approvals = (0..count)
            .into_iter()
            .map(|_| TransactionV1Approval::random(rng))
            .collect();
        FinalizedTransactionV1Approvals(approvals)
    }
}
impl ToBytes for FinalizedTransactionV1Approvals {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for FinalizedTransactionV1Approvals {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (approvals, remainder) = BTreeSet::<TransactionV1Approval>::from_bytes(bytes)?;
        Ok((FinalizedTransactionV1Approvals(approvals), remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let approvals = FinalizedTransactionV1Approvals::random(rng);
        bytesrepr::test_serialization_roundtrip(&approvals);
    }
}
