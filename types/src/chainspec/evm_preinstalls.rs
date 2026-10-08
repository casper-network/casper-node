//! Human-readable encoding of chainspec-supplied EVM runtime bytecode.

use alloc::{collections::BTreeMap, format, string::String};
use core::fmt;

use serde::{
    de::{Error, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};

use crate::{bytesrepr::Bytes, evm::Address};

pub(super) fn serialize<S: Serializer>(
    preinstalls: &BTreeMap<Address, Bytes>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if !serializer.is_human_readable() {
        return preinstalls.serialize(serializer);
    }
    let mut map = serializer.serialize_map(Some(preinstalls.len()))?;
    for (address, code) in preinstalls {
        map.serialize_entry(address, &format!("0x{}", base16::encode_lower(code)))?;
    }
    map.end()
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<Address, Bytes>, D::Error> {
    if !deserializer.is_human_readable() {
        return BTreeMap::deserialize(deserializer);
    }
    deserializer.deserialize_map(PreinstallsVisitor)
}

struct PreinstallsVisitor;

impl<'de> Visitor<'de> for PreinstallsVisitor {
    type Value = BTreeMap<Address, Bytes>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map of EVM addresses to 0x-prefixed runtime bytecode")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut preinstalls = BTreeMap::new();
        while let Some((address, encoded)) = map.next_entry::<Address, String>()? {
            let hex = encoded.strip_prefix("0x").ok_or_else(|| {
                M::Error::custom(format!(
                    "preinstall bytecode at {address} must start with 0x"
                ))
            })?;
            let code = base16::decode(hex.as_bytes()).map_err(M::Error::custom)?;
            if code.is_empty() {
                return Err(M::Error::custom(format!(
                    "preinstall bytecode at {address} must not be empty"
                )));
            }
            if preinstalls.insert(address, Bytes::from(code)).is_some() {
                return Err(M::Error::custom(format!(
                    "duplicate preinstall address {address}"
                )));
            }
        }
        Ok(preinstalls)
    }
}
