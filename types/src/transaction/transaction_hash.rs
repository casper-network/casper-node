use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::TransactionV1;
use super::{DeployHash, TransactionV1Hash};
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

/// A versioned wrapper for a transaction hash or deploy hash.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum TransactionHash {
    /// A deploy hash.
    Deploy(DeployHash),
    /// A version 1 transaction hash.
    #[serde(rename = "Version1")]
    V1(TransactionV1Hash),
}

impl TransactionHash {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            0 => TransactionHash::from(DeployHash::random(rng)),
            1 => TransactionHash::from(TransactionV1Hash::random(rng)),
            _ => panic!(),
        }
    }
}

impl From<DeployHash> for TransactionHash {
    fn from(hash: DeployHash) -> Self {
        Self::Deploy(hash)
    }
}

impl From<&DeployHash> for TransactionHash {
    fn from(hash: &DeployHash) -> Self {
        Self::from(*hash)
    }
}

impl From<TransactionV1Hash> for TransactionHash {
    fn from(hash: TransactionV1Hash) -> Self {
        Self::V1(hash)
    }
}

impl From<&TransactionV1Hash> for TransactionHash {
    fn from(hash: &TransactionV1Hash) -> Self {
        Self::from(*hash)
    }
}

impl Display for TransactionHash {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionHash::Deploy(hash) => Display::fmt(hash, formatter),
            TransactionHash::V1(hash) => Display::fmt(hash, formatter),
        }
    }
}

impl ToBytes for TransactionHash {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionHash::Deploy(hash) => {
                DEPLOY_TAG.write_bytes(writer)?;
                hash.write_bytes(writer)
            }
            TransactionHash::V1(hash) => {
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
                TransactionHash::Deploy(hash) => hash.serialized_length(),
                TransactionHash::V1(hash) => hash.serialized_length(),
            }
    }
}

impl FromBytes for TransactionHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DEPLOY_TAG => {
                let (hash, remainder) = DeployHash::from_bytes(remainder)?;
                Ok((TransactionHash::Deploy(hash), remainder))
            }
            V1_TAG => {
                let (hash, remainder) = TransactionV1Hash::from_bytes(remainder)?;
                Ok((TransactionHash::V1(hash), remainder))
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

        let hash = TransactionHash::from(DeployHash::random(rng));
        bytesrepr::test_serialization_roundtrip(&hash);

        let hash = TransactionHash::from(TransactionV1Hash::random(rng));
        bytesrepr::test_serialization_roundtrip(&hash);
    }
}
