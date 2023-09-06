use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
use super::{TransactionApprovalsHash, TransactionHash};
use crate::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// The unique identifier of a [`Transaction`], comprising its [`TransactionHash`] and
/// [`TransactionApprovalsHash`].
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct TransactionId {
    transaction_hash: TransactionHash,
    approvals_hash: TransactionApprovalsHash,
}

impl TransactionId {
    /// Returns a new `TransactionId`.
    pub fn new(
        transaction_hash: TransactionHash,
        approvals_hash: TransactionApprovalsHash,
    ) -> Self {
        TransactionId {
            transaction_hash,
            approvals_hash,
        }
    }

    /// Returns the transaction hash.
    pub fn transaction_hash(&self) -> &TransactionHash {
        &self.transaction_hash
    }

    /// Returns the approvals hash.
    pub fn approvals_hash(&self) -> &TransactionApprovalsHash {
        &self.approvals_hash
    }

    /// Consumes `self`, returning a tuple of the constituent parts.
    pub fn destructure(self) -> (TransactionHash, TransactionApprovalsHash) {
        (self.transaction_hash, self.approvals_hash)
    }

    /// Returns a random `TransactionId`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TransactionId::new(
            TransactionHash::random(rng),
            TransactionApprovalsHash::random(rng),
        )
    }
}

impl Display for TransactionId {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-id({}, {})",
            self.transaction_hash, self.approvals_hash
        )
    }
}

impl ToBytes for TransactionId {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.transaction_hash.write_bytes(writer)?;
        self.approvals_hash.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.transaction_hash.serialized_length() + self.approvals_hash.serialized_length()
    }
}

impl FromBytes for TransactionId {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (deploy_hash, remainder) = TransactionHash::from_bytes(bytes)?;
        let (approvals_hash, remainder) = TransactionApprovalsHash::from_bytes(remainder)?;
        let id = TransactionId::new(deploy_hash, approvals_hash);
        Ok((id, remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let id = TransactionId::random(rng);
        bytesrepr::test_serialization_roundtrip(&id);
    }
}
