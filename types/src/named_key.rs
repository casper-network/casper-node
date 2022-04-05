// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::{string::String, vec::Vec};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// A named key.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Default, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NamedKey {
    /// The name of the entry.
    pub name: String,
    /// The value of the entry: a casper `Key` type.
    pub key: String,
}

impl ToBytes for NamedKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.name.to_bytes()?);
        buffer.extend(self.key.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length() + self.key.serialized_length()
    }
}

impl FromBytes for NamedKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (name, remainder) = String::from_bytes(bytes)?;
        let (key, remainder) = String::from_bytes(remainder)?;
        let named_key = NamedKey { name, key };
        Ok((named_key, remainder))
    }
}
