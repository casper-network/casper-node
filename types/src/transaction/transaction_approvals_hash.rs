use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::TransactionV1;
use super::{DeployApprovalsHash, TransactionV1ApprovalsHash};
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

/// A versioned wrapper for a transaction approvals hash or deploy approvals hash.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub enum TransactionApprovalsHash {
    /// A deploy approvals hash.
    Deploy(DeployApprovalsHash),
    /// A version 1 transaction approvals hash.
    #[serde(rename = "Version1")]
    V1(TransactionV1ApprovalsHash),
}

impl From<DeployApprovalsHash> for TransactionApprovalsHash {
    fn from(hash: DeployApprovalsHash) -> Self {
        Self::Deploy(hash)
    }
}

impl From<TransactionV1ApprovalsHash> for TransactionApprovalsHash {
    fn from(hash: TransactionV1ApprovalsHash) -> Self {
        Self::V1(hash)
    }
}

impl Display for TransactionApprovalsHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionApprovalsHash::Deploy(hash) => Display::fmt(hash, formatter),
            TransactionApprovalsHash::V1(hash) => Display::fmt(hash, formatter),
        }
    }
}

impl ToBytes for TransactionApprovalsHash {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionApprovalsHash::Deploy(hash) => {
                DEPLOY_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)
            }
            TransactionApprovalsHash::V1(hash) => {
                V1_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)
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
                TransactionApprovalsHash::Deploy(hash) => hash.serialized_length(),
                TransactionApprovalsHash::V1(hash) => hash.serialized_length(),
            }
    }
}

impl FromBytes for TransactionApprovalsHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DEPLOY_TAG => {
                let (hash, remainder) = DeployApprovalsHash::from_bytes(remainder)?;
                Ok((TransactionApprovalsHash::Deploy(hash), remainder))
            }
            V1_TAG => {
                let (hash, remainder) = TransactionV1ApprovalsHash::from_bytes(remainder)?;
                Ok((TransactionApprovalsHash::V1(hash), remainder))
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

        let hash = TransactionApprovalsHash::from(DeployApprovalsHash::random(rng));
        bytesrepr::test_serialization_roundtrip(&hash);

        let hash = TransactionApprovalsHash::from(TransactionV1ApprovalsHash::random(rng));
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
