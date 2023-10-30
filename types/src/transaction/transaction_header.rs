use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use super::{DeployHeader, TransactionV1Header};
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

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
