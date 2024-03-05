use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
use super::{ApprovalsHash, TransactionHash};
use crate::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// The unique identifier of a [`Transaction`], comprising its [`TransactionHash`] and
/// [`ApprovalsHash`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct TransactionId {
    /// The transaction hash.
    transaction_hash: TransactionHash,
    /// The approvals hash.
    approvals_hash: ApprovalsHash,
}

impl TransactionId {
    /// Returns a new `TransactionId`.
    pub fn new(transaction_hash: TransactionHash, approvals_hash: ApprovalsHash) -> Self {
        TransactionId {
            transaction_hash,
            approvals_hash,
        }
    }

    /// Returns the transaction hash.
    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    /// Returns the approvals hash.
    pub fn approvals_hash(&self) -> ApprovalsHash {
        self.approvals_hash
    }

    /// Returns a random `TransactionId`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TransactionId::new(TransactionHash::random(rng), ApprovalsHash::random(rng))
    }
}

impl Display for TransactionId {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-id({}, {})",
            self.transaction_hash(),
            self.approvals_hash()
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
        let (transaction_hash, rem) = TransactionHash::from_bytes(bytes)?;
        let (approvals_hash, rem) = ApprovalsHash::from_bytes(rem)?;
        let transaction_id = TransactionId::new(transaction_hash, approvals_hash);
        Ok((transaction_id, rem))
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
