#[cfg(any(feature = "testing", test))]
use alloc::collections::BTreeSet;

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
#[cfg(any(feature = "testing", test))]
use crate::TransactionApproval;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Transaction,
};

use super::{deploy::FinalizedDeployApprovals, transaction_v1::FinalizedTransactionV1Approvals};

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

/// A set of approvals that has been agreed upon by consensus to approve of a specific transaction.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum FinalizedApprovals {
    /// Finalized approvals for deploys.
    Deploy(FinalizedDeployApprovals),
    /// Finalized approvals for v1 transactions.
    V1(FinalizedTransactionV1Approvals),
}

impl FinalizedApprovals {
    /// Creates a new finalized approvals set.
    pub fn new(transaction: &Transaction) -> Self {
        match transaction {
            Transaction::Deploy(deploy) => {
                Self::Deploy(FinalizedDeployApprovals::new(deploy.approvals().clone()))
            }
            Transaction::V1(txn) => Self::V1(FinalizedTransactionV1Approvals::new(
                txn.approvals().clone(),
            )),
        }
    }

    #[cfg(any(feature = "testing", test))]
    /// Returns the inner set of approvals.
    pub fn inner(&self) -> BTreeSet<TransactionApproval> {
        match self {
            FinalizedApprovals::Deploy(deploy) => deploy
                .inner()
                .iter()
                .map(TransactionApproval::from)
                .collect(),
            FinalizedApprovals::V1(v1) => {
                v1.inner().iter().map(TransactionApproval::from).collect()
            }
        }
    }

    /// Returns a random FinalizedApprovals.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen_bool(0.5) {
            Self::Deploy(FinalizedDeployApprovals::random(rng))
        } else {
            Self::V1(FinalizedTransactionV1Approvals::random(rng))
        }
    }
}

impl From<FinalizedDeployApprovals> for FinalizedApprovals {
    fn from(approvals: FinalizedDeployApprovals) -> Self {
        Self::Deploy(approvals)
    }
}

impl From<FinalizedTransactionV1Approvals> for FinalizedApprovals {
    fn from(approvals: FinalizedTransactionV1Approvals) -> Self {
        Self::V1(approvals)
    }
}

impl ToBytes for FinalizedApprovals {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            FinalizedApprovals::Deploy(approvals) => {
                DEPLOY_TAG.write_bytes(writer)?;
                approvals.write_bytes(writer)
            }
            FinalizedApprovals::V1(approvals) => {
                V1_TAG.write_bytes(writer)?;
                approvals.write_bytes(writer)
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
                FinalizedApprovals::Deploy(approvals) => approvals.serialized_length(),
                FinalizedApprovals::V1(approvals) => approvals.serialized_length(),
            }
    }
}

impl FromBytes for FinalizedApprovals {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DEPLOY_TAG => {
                let (approvals, remainder) = FinalizedDeployApprovals::from_bytes(remainder)?;
                Ok((FinalizedApprovals::Deploy(approvals), remainder))
            }
            V1_TAG => {
                let (approvals, remainder) =
                    FinalizedTransactionV1Approvals::from_bytes(remainder)?;
                Ok((FinalizedApprovals::V1(approvals), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let approvals = FinalizedApprovals::from(FinalizedDeployApprovals::random(rng));
        bytesrepr::test_serialization_roundtrip(&approvals);

        let approvals = FinalizedApprovals::from(FinalizedTransactionV1Approvals::random(rng));
        bytesrepr::test_serialization_roundtrip(&approvals);
    }
}
