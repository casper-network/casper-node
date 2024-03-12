use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::TransformKindV2;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Key,
};

/// A transformation performed while executing a deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TransformV2 {
    key: Key,
    kind: TransformKindV2,
}

impl TransformV2 {
    /// Constructs a new `Transform`.
    pub fn new(key: Key, kind: TransformKindV2) -> Self {
        TransformV2 { key, kind }
    }

    /// Returns the key whose value was transformed.
    pub fn key(&self) -> &Key {
        &self.key
    }

    /// Returns the transformation kind.
    pub fn kind(&self) -> &TransformKindV2 {
        &self.kind
    }

    /// Consumes `self`, returning its constituent parts.
    pub fn destructure(self) -> (Key, TransformKindV2) {
        (self.key, self.kind)
    }
}

impl ToBytes for TransformV2 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.key.write_bytes(writer)?;
        self.kind.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.key.serialized_length() + self.kind.serialized_length()
    }
}

impl FromBytes for TransformV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, remainder) = Key::from_bytes(bytes)?;
        let (transform, remainder) = TransformKindV2::from_bytes(remainder)?;
        let transform_entry = TransformV2 {
            key,
            kind: transform,
        };
        Ok((transform_entry, remainder))
    }
}
