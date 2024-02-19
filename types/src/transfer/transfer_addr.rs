use alloc::{string::String, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    transfer_v1::V1_PREFIX, transfer_v2::V2_PREFIX, TransferFromStrError, TransferV1Addr,
    TransferV2Addr, TRANSFER_V1_ADDR_LENGTH, TRANSFER_V2_ADDR_LENGTH,
};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex,
};

pub(super) const TRANSFER_ADDR_FORMATTED_STRING_PREFIX: &str = "transfer-";
const V1_TAG: u8 = 0;
const V2_TAG: u8 = 1;

/// A versioned wrapper for a transfer address.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum TransferAddr {
    /// A version 1 transfer address.
    #[serde(rename = "Version1")]
    V1(TransferV1Addr),
    /// A version 2 transfer address.
    #[serde(rename = "Version2")]
    V2(TransferV2Addr),
}

impl TransferAddr {
    /// Formats the `TransferAddr` as a prefixed, hex-encoded string.
    pub fn to_formatted_string(self) -> String {
        match self {
            TransferAddr::V1(addr) => addr.to_formatted_string(),
            TransferAddr::V2(addr) => addr.to_formatted_string(),
        }
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `TransferAddr`.
    pub fn from_formatted_str(input: &str) -> Result<Self, TransferFromStrError> {
        let v_remainder = input
            .strip_prefix(TRANSFER_ADDR_FORMATTED_STRING_PREFIX)
            .ok_or(TransferFromStrError::InvalidPrefix)?;
        if let Some(v1_addr_hex) = v_remainder.strip_prefix(V1_PREFIX) {
            let addr = <[u8; TRANSFER_V1_ADDR_LENGTH]>::try_from(
                checksummed_hex::decode(v1_addr_hex)?.as_ref(),
            )?;
            Ok(TransferAddr::V1(TransferV1Addr::new(addr)))
        } else if let Some(v2_addr_hex) = v_remainder.strip_prefix(V2_PREFIX) {
            let addr = <[u8; TRANSFER_V2_ADDR_LENGTH]>::try_from(
                checksummed_hex::decode(v2_addr_hex)?.as_ref(),
            )?;
            Ok(TransferAddr::V2(TransferV2Addr::new(addr)))
        } else {
            Err(TransferFromStrError::InvalidPrefix)
        }
    }

    /// Returns a random `TransferAddr`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            TransferAddr::V1(TransferV1Addr::random(rng))
        } else {
            TransferAddr::V2(TransferV2Addr::random(rng))
        }
    }
}

impl From<TransferV1Addr> for TransferAddr {
    fn from(addr: TransferV1Addr) -> Self {
        Self::V1(addr)
    }
}

impl From<&TransferV1Addr> for TransferAddr {
    fn from(addr: &TransferV1Addr) -> Self {
        Self::from(*addr)
    }
}

impl From<TransferV2Addr> for TransferAddr {
    fn from(addr: TransferV2Addr) -> Self {
        Self::V2(addr)
    }
}

impl From<&TransferV2Addr> for TransferAddr {
    fn from(addr: &TransferV2Addr) -> Self {
        Self::from(*addr)
    }
}

impl Display for TransferAddr {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransferAddr::V1(addr) => Display::fmt(addr, formatter),
            TransferAddr::V2(addr) => Display::fmt(addr, formatter),
        }
    }
}

impl ToBytes for TransferAddr {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransferAddr::V1(addr) => {
                V1_TAG.write_bytes(writer)?;
                addr.write_bytes(writer)
            }
            TransferAddr::V2(addr) => {
                V2_TAG.write_bytes(writer)?;
                addr.write_bytes(writer)
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
                TransferAddr::V1(addr) => addr.serialized_length(),
                TransferAddr::V2(addr) => addr.serialized_length(),
            }
    }
}

impl FromBytes for TransferAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            V1_TAG => {
                let (addr, remainder) = TransferV1Addr::from_bytes(remainder)?;
                Ok((TransferAddr::V1(addr), remainder))
            }
            V2_TAG => {
                let (addr, remainder) = TransferV2Addr::from_bytes(remainder)?;
                Ok((TransferAddr::V2(addr), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<TransferAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> TransferAddr {
        if rng.gen() {
            TransferAddr::V1(TransferV1Addr::new(rng.gen()))
        } else {
            TransferAddr::V2(TransferV2Addr::new(rng.gen()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn transfer_addr_from_str() {
        let transfer_v1_addr = TransferV1Addr::new([4; 32]);
        let encoded = transfer_v1_addr.to_formatted_string();
        let decoded = TransferV1Addr::from_formatted_str(&encoded).unwrap();
        assert_eq!(transfer_v1_addr, decoded);

        let transfer_v2_addr = TransferV2Addr::new([4; 32]);
        let encoded = transfer_v2_addr.to_formatted_string();
        let decoded = TransferV2Addr::from_formatted_str(&encoded).unwrap();
        assert_eq!(transfer_v2_addr, decoded);

        let invalid_prefix =
            "transfer-v-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV2Addr::from_formatted_str(invalid_prefix),
            Err(TransferFromStrError::InvalidPrefix)
        ));

        let invalid_prefix =
            "transfer-v20000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV2Addr::from_formatted_str(invalid_prefix),
            Err(TransferFromStrError::InvalidPrefix)
        ));

        let short_addr =
            "transfer-v2-00000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV2Addr::from_formatted_str(short_addr),
            Err(TransferFromStrError::Length(_))
        ));

        let long_addr =
            "transfer-v2-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV2Addr::from_formatted_str(long_addr),
            Err(TransferFromStrError::Length(_))
        ));

        let invalid_hex =
            "transfer-v2-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(matches!(
            TransferV2Addr::from_formatted_str(invalid_hex),
            Err(TransferFromStrError::Hex(_))
        ));
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let addr = TransferAddr::random(rng);
        bytesrepr::test_serialization_roundtrip(&addr);
    }

    #[test]
    fn bincode_roundtrip() {
        let rng = &mut TestRng::new();
        let addr = TransferAddr::random(rng);
        let serialized = bincode::serialize(&addr).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(addr, decoded);
    }

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();
        let addr = TransferAddr::random(rng);
        let json_string = serde_json::to_string_pretty(&addr).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(addr, decoded);
    }
}
