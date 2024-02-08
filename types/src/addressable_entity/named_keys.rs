use alloc::{collections::BTreeMap, string::String, vec::Vec};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(feature = "json-schema")]
use serde_map_to_array::KeyValueJsonSchema;
use serde_map_to_array::{BTreeMapToArray, KeyValueLabels};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, Key,
};

/// A collection of named keys.
#[derive(Clone, Eq, PartialEq, Default, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
pub struct NamedKeys(
    #[serde(with = "BTreeMapToArray::<String, Key, Labels>")]
    BTreeMap<String, Key>,
);

impl NamedKeys {
    /// Constructs a new, empty `NamedKeys`.
    pub const fn new() -> Self {
        NamedKeys(BTreeMap::new())
    }

    /// Consumes `self`, returning the wrapped map.
    pub fn into_inner(self) -> BTreeMap<String, Key> {
        self.0
    }

    /// Inserts a named key.
    ///
    /// If the map did not have this name present, `None` is returned.  If the map did have this
    /// name present, the `Key` is updated, and the old `Key` is returned.
    pub fn insert(&mut self, name: String, key: Key) -> Option<Key> {
        self.0.insert(name, key)
    }

    /// Moves all elements from `other` into `self`.
    pub fn append(&mut self, mut other: Self) {
        self.0.append(&mut other.0)
    }

    /// Removes a named `Key`, returning the `Key` if it existed in the collection.
    pub fn remove(&mut self, name: &str) -> Option<Key> {
        self.0.remove(name)
    }

    /// Returns a reference to the `Key` under the given `name` if any.
    pub fn get(&self, name: &str) -> Option<&Key> {
        self.0.get(name)
    }

    /// Returns `true` if the named `Key` exists in the collection.
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Returns an iterator over the names.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    /// Returns an iterator over the `Key`s (i.e. the map's values).
    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.0.values()
    }

    /// Returns a mutable iterator over the `Key`s (i.e. the map's values).
    pub fn keys_mut(&mut self) -> impl Iterator<Item = &mut Key> {
        self.0.values_mut()
    }

    /// Returns an iterator over the name-key pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Key)> {
        self.0.iter()
    }

    /// Returns the number of named `Key`s.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if there are no named `Key`s.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<BTreeMap<String, Key>> for NamedKeys {
    fn from(value: BTreeMap<String, Key>) -> Self {
        NamedKeys(value)
    }
}

impl ToBytes for NamedKeys {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for NamedKeys {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (named_keys, remainder) = BTreeMap::<String, Key>::from_bytes(bytes)?;
        Ok((NamedKeys(named_keys), remainder))
    }
}

impl CLTyped for NamedKeys {
    fn cl_type() -> CLType {
        BTreeMap::<String, Key>::cl_type()
    }
}

struct Labels;

impl KeyValueLabels for Labels {
    const KEY: &'static str = "name";
    const VALUE: &'static str = "key";
}

#[cfg(feature = "json-schema")]
impl KeyValueJsonSchema for Labels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("NamedKey");
    const JSON_SCHEMA_KV_DESCRIPTION: Option<&'static str> = Some("A key with a name.");
    const JSON_SCHEMA_KEY_DESCRIPTION: Option<&'static str> = Some("The name of the entry.");
    const JSON_SCHEMA_VALUE_DESCRIPTION: Option<&'static str> =
        Some("The value of the entry: a casper `Key` type.");
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;
    use crate::testing::TestRng;

    /// `NamedKeys` was previously (pre node v2.0.0) just an alias for `BTreeMap<String, Key>`.
    /// Check if we serialize as the old form, that can deserialize to the new.
    #[test]
    fn should_be_backwards_compatible() {
        let rng = &mut TestRng::new();
        let mut named_keys = NamedKeys::new();
        assert!(named_keys.insert("a".to_string(), rng.gen()).is_none());
        assert!(named_keys.insert("bb".to_string(), rng.gen()).is_none());
        assert!(named_keys.insert("ccc".to_string(), rng.gen()).is_none());

        let serialized_old = bincode::serialize(&named_keys.0).unwrap();
        let parsed_new = bincode::deserialize(&serialized_old).unwrap();
        assert_eq!(named_keys, parsed_new);

        let serialized_old = bytesrepr::serialize(&named_keys.0).unwrap();
        let parsed_new = bytesrepr::deserialize(serialized_old).unwrap();
        assert_eq!(named_keys, parsed_new);
    }
}
