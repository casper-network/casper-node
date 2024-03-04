use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "std", test))]
use crate::{TimeDiff, TransactionConfig, TransactionHash};

use super::{DeployHeader, TransactionV1Header};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest, Timestamp,
};

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

/// A versioned wrapper for a transaction header or deploy header.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum TransactionHeader {
    /// A deploy header.
    Deploy(DeployHeader),
    /// A version 1 transaction header.
    #[cfg_attr(any(feature = "std", test), serde(rename = "Version1"))]
    V1(TransactionV1Header),
}

impl TransactionHeader {
    /// Returns the body hash of the transaction.
    pub fn body_hash(&self) -> &Digest {
        match self {
            TransactionHeader::Deploy(header) => header.body_hash(),
            TransactionHeader::V1(header) => header.body_hash(),
        }
    }

    /// Returns `true` if the `Transaction` has expired.
    pub fn expired(&self, current_time: Timestamp) -> bool {
        match self {
            TransactionHeader::Deploy(header) => header.expired(current_time),
            TransactionHeader::V1(header) => header.expired(current_time),
        }
    }

    /// Returns `true` if the `Transaction` is valid.
    #[cfg(any(feature = "std", test))]
    pub fn is_valid(
        &self,
        config: &TransactionConfig,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
        transaction_hash: &TransactionHash,
    ) -> bool {
        match self {
            TransactionHeader::Deploy(header) => header
                .is_valid(
                    config,
                    timestamp_leeway,
                    at,
                    match transaction_hash {
                        TransactionHash::Deploy(hash) => hash,
                        _ => panic!("expected deploy hash"),
                    },
                )
                .is_ok(),
            TransactionHeader::V1(header) => header
                .is_valid(config, timestamp_leeway, at, transaction_hash)
                .is_ok(),
        }
    }
}

impl From<DeployHeader> for TransactionHeader {
    fn from(hash: DeployHeader) -> Self {
        Self::Deploy(hash)
    }
}

impl From<TransactionV1Header> for TransactionHeader {
    fn from(hash: TransactionV1Header) -> Self {
        Self::V1(hash)
    }
}

impl Display for TransactionHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionHeader::Deploy(hash) => Display::fmt(hash, formatter),
            TransactionHeader::V1(hash) => Display::fmt(hash, formatter),
        }
    }
}

impl ToBytes for TransactionHeader {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionHeader::Deploy(header) => {
                DEPLOY_TAG.write_bytes(writer)?;
                header.write_bytes(writer)
            }
            TransactionHeader::V1(header) => {
                V1_TAG.write_bytes(writer)?;
                header.write_bytes(writer)
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
                TransactionHeader::Deploy(header) => header.serialized_length(),
                TransactionHeader::V1(header) => header.serialized_length(),
            }
    }
}

impl FromBytes for TransactionHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            DEPLOY_TAG => {
                let (header, remainder) = DeployHeader::from_bytes(remainder)?;
                Ok((TransactionHeader::Deploy(header), remainder))
            }
            V1_TAG => {
                let (header, remainder) = TransactionV1Header::from_bytes(remainder)?;
                Ok((TransactionHeader::V1(header), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{testing::TestRng, Deploy, TransactionV1};

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let header = TransactionHeader::from(Deploy::random(rng).take_header());
        bytesrepr::test_serialization_roundtrip(&header);

        let header = TransactionHeader::from(TransactionV1::random(rng).take_header());
        bytesrepr::test_serialization_roundtrip(&header);
    }
}
