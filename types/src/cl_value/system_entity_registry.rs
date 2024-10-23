//! The registry of system contracts.

use alloc::{collections::BTreeMap, string::String, vec::Vec};

// #[cfg(feature = "datasize")]
// use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    system::STANDARD_PAYMENT,
    AddressableEntityHash, CLType, CLTyped, HashAddr,
};

/// The system entity registry.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
pub struct SystemHashRegistry(BTreeMap<String, HashAddr>);

impl SystemHashRegistry {
    /// Returns a new `SystemEntityRegistry`.
    #[allow(clippy::new_without_default)] // This empty `new()` will be replaced in the future.
    pub fn new() -> Self {
        SystemHashRegistry(BTreeMap::new())
    }

    /// Inserts a contract's details into the registry.
    pub fn insert(&mut self, contract_name: String, contract_hash: HashAddr) {
        self.0.insert(contract_name, contract_hash);
    }

    /// Gets a contract's hash from the registry.
    pub fn get(&self, contract_name: &str) -> Option<&HashAddr> {
        self.0.get(contract_name)
    }

    /// Returns `true` if the given hash_addr exists as a value in the registry.
    pub fn exists(&self, hash_addr: &HashAddr) -> bool {
        self.0
            .values()
            .any(|system_contract_hash| system_contract_hash == hash_addr)
    }

    /// Remove standard payment from the contract registry.
    pub fn remove_standard_payment(&mut self) -> Option<HashAddr> {
        self.0.remove(STANDARD_PAYMENT)
    }

    pub fn inner(self) -> BTreeMap<String, HashAddr> {
        self.0
    }
}

impl ToBytes for SystemHashRegistry {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for SystemHashRegistry {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = BTreeMap::from_bytes(bytes)?;
        Ok((SystemHashRegistry(inner), remainder))
    }
}

impl CLTyped for SystemHashRegistry {
    fn cl_type() -> CLType {
        BTreeMap::<String, AddressableEntityHash>::cl_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut system_entity_registry = SystemHashRegistry::new();
        system_entity_registry.insert("a".to_string(), [9; 32]);
        bytesrepr::test_serialization_roundtrip(&system_entity_registry);
    }

    #[test]
    fn bytesrepr_transparent() {
        // this test ensures that the serialization is not affected by the wrapper, because
        // this data is deserialized in other places as a BTree, e.g. GetAuctionInfo in the sidecar
        let mut system_entity_registry = SystemHashRegistry::new();
        system_entity_registry.insert("a".to_string(), [9; 32]);
        let serialized =
            ToBytes::to_bytes(&system_entity_registry).expect("Unable to serialize data");
        let deserialized: BTreeMap<String, HashAddr> =
            bytesrepr::deserialize_from_slice(serialized).expect("Unable to deserialize data");
        assert_eq!(system_entity_registry, SystemHashRegistry(deserialized));
    }
}
