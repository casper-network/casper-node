//! The registry of checksums.

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, Digest,
};

/// The checksum registry.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug, Default)]
pub struct ChecksumRegistry(BTreeMap<String, Digest>);

impl ChecksumRegistry {
    /// Returns a new `ChecksumRegistry`.
    pub fn new() -> Self {
        ChecksumRegistry(BTreeMap::new())
    }

    /// Inserts a checksum into the registry.
    pub fn insert(&mut self, checksum_name: &str, checksum: Digest) {
        self.0.insert(checksum_name.to_string(), checksum);
    }

    /// Gets a checksum from the registry.
    pub fn get(&self, checksum_name: &str) -> Option<&Digest> {
        self.0.get(checksum_name)
    }
}

impl ToBytes for ChecksumRegistry {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for ChecksumRegistry {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = BTreeMap::from_bytes(bytes)?;
        Ok((ChecksumRegistry(inner), remainder))
    }
}

impl CLTyped for ChecksumRegistry {
    fn cl_type() -> CLType {
        BTreeMap::<String, Digest>::cl_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut checksum_registry = ChecksumRegistry::new();
        checksum_registry.insert("a", Digest::hash([9; 100]));
        bytesrepr::test_serialization_roundtrip(&checksum_registry);
    }
}
