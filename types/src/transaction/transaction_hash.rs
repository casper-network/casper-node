use alloc::{string::String, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{DeployHash, TransactionV1Hash};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest,
};

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;
const TAG_LENGTH: u8 = 1;

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
    /// The number of bytes in a `DeployHash` digest.
    pub const LENGTH: usize = TAG_LENGTH as usize + Digest::LENGTH;
    /// Digest representation of hash.
    pub fn digest(&self) -> Digest {
        match self {
            TransactionHash::Deploy(deploy_hash) => *deploy_hash.inner(),
            TransactionHash::V1(transaction_hash) => *transaction_hash.inner(),
        }
    }

    /// Hexadecimal representation of the hash.
    pub fn to_hex_string(&self) -> String {
        base16::encode_lower(&self.digest())
    }

    /// Returns a random `TransactionHash`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            0 => TransactionHash::from(DeployHash::random(rng)),
            1 => TransactionHash::from(TransactionV1Hash::random(rng)),
            _ => panic!(),
        }
    }

    /// Returns a new `TransactionHash` directly initialized with the provided bytes; no hashing
    /// is done.
    pub const fn from_raw(raw_digest: [u8; TransactionV1Hash::LENGTH]) -> Self {
        TransactionHash::V1(TransactionV1Hash::from_raw(raw_digest))
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

impl Default for TransactionHash {
    fn default() -> Self {
        TransactionHash::V1(TransactionV1Hash::default())
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

impl AsRef<[u8]> for TransactionHash {
    fn as_ref(&self) -> &[u8] {
        match self {
            TransactionHash::Deploy(hash) => hash.as_ref(),
            TransactionHash::V1(hash) => hash.as_ref(),
        }
    }
}

impl ToBytes for TransactionHash {
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
