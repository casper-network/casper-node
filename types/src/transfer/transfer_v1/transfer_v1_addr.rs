use alloc::{format, string::String, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use super::super::TransferFromStrError;
pub(super) const TRANSFER_ADDR_FORMATTED_STRING_PREFIX: &str = "transfer-";
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    checksummed_hex, CLType, CLTyped,
};

/// The length of a version 1 transfer address.
pub const TRANSFER_V1_ADDR_LENGTH: usize = 32;
pub(in crate::transfer) const V1_PREFIX: &str = "v1-";

/// A newtype wrapping a <code>[u8; [TRANSFER_V1_ADDR_LENGTH]]</code> which is the raw bytes of the
/// transfer address.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Default)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Hex-encoded version 1 transfer address.")
)]
pub struct TransferV1Addr(
    #[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))]
    [u8; TRANSFER_V1_ADDR_LENGTH],
);

impl TransferV1Addr {
    /// Constructs a new `TransferV1Addr` instance from the raw bytes.
    pub const fn new(value: [u8; TRANSFER_V1_ADDR_LENGTH]) -> TransferV1Addr {
        TransferV1Addr(value)
    }

    /// Returns the raw bytes of the transfer address as an array.
    pub fn value(&self) -> [u8; TRANSFER_V1_ADDR_LENGTH] {
        self.0
    }

    /// Returns the raw bytes of the transfer address as a `slice`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Formats the `TransferV1Addr` as a prefixed, hex-encoded string.
    pub fn to_formatted_string(self) -> String {
        format!(
            "{}{}{}",
            TRANSFER_ADDR_FORMATTED_STRING_PREFIX,
            V1_PREFIX,
            base16::encode_lower(&self.0),
        )
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `TransferV1Addr`.
    pub fn from_formatted_str(input: &str) -> Result<Self, TransferFromStrError> {
        let v1_remainder = input
            .strip_prefix(TRANSFER_ADDR_FORMATTED_STRING_PREFIX)
            .ok_or(TransferFromStrError::InvalidPrefix)?;
        let remainder = v1_remainder
            .strip_prefix(V1_PREFIX)
            .ok_or(TransferFromStrError::InvalidPrefix)?;
        let bytes = <[u8; TRANSFER_V1_ADDR_LENGTH]>::try_from(
            checksummed_hex::decode(remainder)?.as_ref(),
        )?;
        Ok(TransferV1Addr(bytes))
    }

    /// Returns a random `TransferV1Addr`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TransferV1Addr(rng.gen())
    }
}

impl Serialize for TransferV1Addr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for TransferV1Addr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            TransferV1Addr::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = <[u8; TRANSFER_V1_ADDR_LENGTH]>::deserialize(deserializer)?;
            Ok(TransferV1Addr(bytes))
        }
    }
}

impl Display for TransferV1Addr {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for TransferV1Addr {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "TransferV1Addr({})", base16::encode_lower(&self.0))
    }
}

impl CLTyped for TransferV1Addr {
    fn cl_type() -> CLType {
        CLType::ByteArray(TRANSFER_V1_ADDR_LENGTH as u32)
    }
}

impl ToBytes for TransferV1Addr {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    #[inline(always)]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for TransferV1Addr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bytes, remainder) = <[u8; TRANSFER_V1_ADDR_LENGTH]>::from_bytes(bytes)?;
        Ok((TransferV1Addr(bytes), remainder))
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, testing::TestRng};

    use super::*;

    #[test]
    fn transfer_addr_from_str() {
        let transfer_address = TransferV1Addr([4; 32]);
        let encoded = transfer_address.to_formatted_string();
        let decoded = TransferV1Addr::from_formatted_str(&encoded).unwrap();
        assert_eq!(transfer_address, decoded);

        let invalid_prefix =
            "transfer-v-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV1Addr::from_formatted_str(invalid_prefix),
            Err(TransferFromStrError::InvalidPrefix)
        ));

        let invalid_prefix =
            "transfer-v10000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV1Addr::from_formatted_str(invalid_prefix),
            Err(TransferFromStrError::InvalidPrefix)
        ));

        let short_addr =
            "transfer-v1-00000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV1Addr::from_formatted_str(short_addr),
            Err(TransferFromStrError::Length(_))
        ));

        let long_addr =
            "transfer-v1-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            TransferV1Addr::from_formatted_str(long_addr),
            Err(TransferFromStrError::Length(_))
        ));

        let invalid_hex =
            "transfer-v1-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(matches!(
            TransferV1Addr::from_formatted_str(invalid_hex),
            Err(TransferFromStrError::Hex(_))
        ));
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let transfer_address = TransferV1Addr::random(rng);
        bytesrepr::test_serialization_roundtrip(&transfer_address)
    }

    #[test]
    fn bincode_roundtrip() {
        let rng = &mut TestRng::new();
        let transfer_address = TransferV1Addr::random(rng);
        let serialized = bincode::serialize(&transfer_address).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(transfer_address, decoded);
    }

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();
        let transfer_address = TransferV1Addr::random(rng);
        let json_string = serde_json::to_string_pretty(&transfer_address).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(transfer_address, decoded);
    }
}
