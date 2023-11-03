use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
use super::{
    DeployApprovalsHash, DeployHash, TransactionApprovalsHash, TransactionHash,
    TransactionV1ApprovalsHash, TransactionV1Hash,
};
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

/// The unique identifier of a [`Transaction`], comprising its [`TransactionHash`] and
/// [`TransactionApprovalsHash`].
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub enum TransactionId {
    /// A deploy identifier.
    Deploy {
        /// The deploy hash.
        deploy_hash: DeployHash,
        /// The deploy's approvals hash.
        approvals_hash: DeployApprovalsHash,
    },
    /// A version 1 transaction identifier.
    #[serde(rename = "Version1")]
    V1 {
        /// The transaction hash.
        transaction_v1_hash: TransactionV1Hash,
        /// The transaction's approvals hash.
        approvals_hash: TransactionV1ApprovalsHash,
    },
}

impl TransactionId {
    /// Returns a new `TransactionId::Deploy`.
    pub fn new_deploy(deploy_hash: DeployHash, approvals_hash: DeployApprovalsHash) -> Self {
        TransactionId::Deploy {
            deploy_hash,
            approvals_hash,
        }
    }

    /// Returns a new `TransactionId::V1`.
    pub fn new_v1(
        transaction_v1_hash: TransactionV1Hash,
        approvals_hash: TransactionV1ApprovalsHash,
    ) -> Self {
        TransactionId::V1 {
            transaction_v1_hash,
            approvals_hash,
        }
    }

    /// Returns the transaction hash.
    pub fn transaction_hash(&self) -> TransactionHash {
        match self {
            TransactionId::Deploy { deploy_hash, .. } => TransactionHash::from(*deploy_hash),
            TransactionId::V1 {
                transaction_v1_hash,
                ..
            } => TransactionHash::from(*transaction_v1_hash),
        }
    }

    /// Returns the approvals hash.
    pub fn approvals_hash(&self) -> TransactionApprovalsHash {
        match self {
            TransactionId::Deploy { approvals_hash, .. } => {
                TransactionApprovalsHash::from(*approvals_hash)
            }
            TransactionId::V1 { approvals_hash, .. } => {
                TransactionApprovalsHash::from(*approvals_hash)
            }
        }
    }

    /// Returns a random `TransactionId`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            return TransactionId::new_deploy(
                DeployHash::random(rng),
                DeployApprovalsHash::random(rng),
            );
        }
        TransactionId::new_v1(
            TransactionV1Hash::random(rng),
            TransactionV1ApprovalsHash::random(rng),
        )
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
        match self {
            TransactionId::Deploy {
                deploy_hash,
                approvals_hash,
            } => {
                DEPLOY_TAG.write_bytes(writer)?;
                deploy_hash.write_bytes(writer)?;
                approvals_hash.write_bytes(writer)
            }
            TransactionId::V1 {
                transaction_v1_hash,
                approvals_hash,
            } => {
                V1_TAG.write_bytes(writer)?;
                transaction_v1_hash.write_bytes(writer)?;
                approvals_hash.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TransactionId::Deploy {
                    deploy_hash,
                    approvals_hash,
                } => deploy_hash.serialized_length() + approvals_hash.serialized_length(),
                TransactionId::V1 {
                    transaction_v1_hash,
                    approvals_hash,
                } => transaction_v1_hash.serialized_length() + approvals_hash.serialized_length(),
            }
    }
}

impl FromBytes for TransactionId {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DEPLOY_TAG => {
                let (deploy_hash, remainder) = DeployHash::from_bytes(remainder)?;
                let (approvals_hash, remainder) = DeployApprovalsHash::from_bytes(remainder)?;
                let id = TransactionId::Deploy {
                    deploy_hash,
                    approvals_hash,
                };
                Ok((id, remainder))
            }
            V1_TAG => {
                let (transaction_v1_hash, remainder) = TransactionV1Hash::from_bytes(remainder)?;
                let (approvals_hash, remainder) =
                    TransactionV1ApprovalsHash::from_bytes(remainder)?;
                let id = TransactionId::V1 {
                    transaction_v1_hash,
                    approvals_hash,
                };
                Ok((id, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
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
