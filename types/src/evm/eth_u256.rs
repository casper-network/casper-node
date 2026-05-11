use alloc::{
    format,
    string::{String, ToString},
};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::U256;

const ETH_U256_BYTES: usize = 32;

/// A 256-bit unsigned integer encoded as an Ethereum JSON-RPC quantity.
///
/// Human-readable serializers use canonical `0x`-prefixed hexadecimal without
/// leading zeroes. Binary serializers delegate to [`U256`].
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct EthU256(U256);

impl EthU256 {
    /// The zero quantity.
    pub const ZERO: EthU256 = EthU256(U256::MIN);

    /// Returns the underlying 256-bit integer.
    pub fn value(self) -> U256 {
        self.0
    }

    /// Converts this value into a `u64` if it fits.
    pub fn as_u64(self) -> Result<u64, &'static str> {
        if self.0 > U256::from(u64::MAX) {
            Err("quantity exceeds u64")
        } else {
            Ok(self.0.low_u64())
        }
    }

    fn to_quantity_hex(self) -> String {
        if self.0.is_zero() {
            return "0x0".to_string();
        }
        let mut bytes = [0u8; ETH_U256_BYTES];
        self.0.to_big_endian(&mut bytes);
        let first_non_zero = bytes
            .iter()
            .position(|byte| *byte != 0)
            .expect("non-zero U256 has a non-zero byte");
        let hex = base16::encode_lower(&bytes[first_non_zero..]);
        format!("0x{}", hex.trim_start_matches('0'))
    }
}

impl From<U256> for EthU256 {
    fn from(value: U256) -> Self {
        EthU256(value)
    }
}

impl From<EthU256> for U256 {
    fn from(value: EthU256) -> Self {
        value.value()
    }
}

impl From<u8> for EthU256 {
    fn from(value: u8) -> Self {
        EthU256(U256::from(value))
    }
}

impl From<u64> for EthU256 {
    fn from(value: u64) -> Self {
        EthU256(U256::from(value))
    }
}

impl From<u128> for EthU256 {
    fn from(value: u128) -> Self {
        EthU256(U256::from(value))
    }
}

impl From<usize> for EthU256 {
    fn from(value: usize) -> Self {
        EthU256(U256::from(value))
    }
}

impl Display for EthU256 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_quantity_hex())
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for EthU256 {
    fn schema_name() -> String {
        String::from("EthU256")
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description =
            Some("Ethereum JSON-RPC quantity encoded as canonical 0x-prefixed hexadecimal.".into());
        schema_object.into()
    }
}

impl Serialize for EthU256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.collect_str(self)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for EthU256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let value = String::deserialize(deserializer)?;
            parse_eth_u256(&value)
                .map(EthU256)
                .map_err(D::Error::custom)
        } else {
            U256::deserialize(deserializer).map(EthU256)
        }
    }
}

fn parse_eth_u256(value: &str) -> Result<U256, String> {
    let hex = value
        .strip_prefix("0x")
        .ok_or_else(|| "quantity must start with 0x".to_string())?;
    if hex.is_empty() {
        return Err("quantity must not be empty".to_string());
    }
    if hex.len() > ETH_U256_BYTES * 2 {
        return Err("quantity exceeds 256 bits".to_string());
    }
    let padded = if hex.len() % 2 == 0 {
        hex.to_string()
    } else {
        format!("0{hex}")
    };
    let bytes = base16::decode(padded.as_bytes()).map_err(|error| format!("{error}"))?;
    Ok(U256::from_big_endian(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_readable_serde_uses_canonical_ethereum_quantity_hex() {
        let value = EthU256::from(U256::from_big_endian(&[0x01, 0x23]));

        let encoded = serde_json::to_string(&value).expect("quantity should serialize");
        assert_eq!(encoded, "\"0x123\"");

        let decoded: EthU256 = serde_json::from_str(&encoded).expect("quantity should deserialize");
        assert_eq!(decoded, value);
    }

    #[test]
    fn zero_serializes_as_zero_quantity() {
        let encoded = serde_json::to_string(&EthU256::ZERO).expect("quantity should serialize");
        assert_eq!(encoded, "\"0x0\"");
    }

    #[test]
    fn human_readable_serde_accepts_full_width_quantity() {
        let encoded = format!("\"0x{}\"", "f".repeat(ETH_U256_BYTES * 2));
        let decoded: EthU256 = serde_json::from_str(&encoded).expect("quantity should deserialize");

        assert_eq!(decoded.value(), U256::MAX);
    }

    #[test]
    fn human_readable_serde_rejects_invalid_quantities() {
        for encoded in [
            "\"1\"",
            "\"0x\"",
            "\"0xg\"",
            "\"0x10000000000000000000000000000000000000000000000000000000000000000\"",
        ] {
            serde_json::from_str::<EthU256>(encoded)
                .expect_err("invalid quantity should fail to deserialize");
        }
    }

    #[test]
    fn as_u64_rejects_overflow() {
        assert_eq!(EthU256::from(u64::MAX).as_u64(), Ok(u64::MAX));
        assert_eq!(
            EthU256::from(U256::from(u64::MAX) + U256::from(1)).as_u64(),
            Err("quantity exceeds u64")
        );
    }

    #[test]
    fn non_human_readable_serde_roundtrip() {
        let value = EthU256::from(U256::from_big_endian(&[0xab; ETH_U256_BYTES]));
        let encoded = bincode::serialize(&value).expect("quantity should serialize");
        let decoded: EthU256 = bincode::deserialize(&encoded).expect("quantity should deserialize");

        assert_eq!(decoded, value);
    }
}
