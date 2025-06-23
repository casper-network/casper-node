#[cfg(not(target_arch = "wasm32"))]
use crate::abi::{CasperABI, Declaration, StructField, TypeDef};
use crate::{
    casper,
    common::type_uid::{TypeUid, Uid},
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};
use casper_executor_wasm_common::keyspace::Keyspace;
use const_fnv1a_hash::fnv1a_hash_str_64;

use crate::prelude::marker::PhantomData;

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct Map<K, V> {
    pub(crate) name: String,
    pub(crate) _marker: PhantomData<(K, V)>,
}

/// Computes the prefix for a given key.
#[allow(dead_code)]
pub(crate) const fn compute_prefix(input: &str) -> [u8; 8] {
    let hash = fnv1a_hash_str_64(input);
    hash.to_le_bytes()
}

impl<K, V> Map<K, V>
where
    K: BorshSerialize,
    V: BorshSerialize + BorshDeserialize + TypeUid,
{
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self {
            name: name.into(),
            _marker: PhantomData,
        }
    }

    pub fn insert(&mut self, key: &K, value: &V) {
        let mut context_key = Vec::new();
        context_key.extend(self.name.as_bytes());
        // NOTE: We may want to create new keyspace for a hashed context element to avoid hashing in
        // the wasm.
        key.serialize(&mut context_key).unwrap();
        let prefix = Keyspace::Context(&context_key);
        casper::write(prefix, value).unwrap();
    }

    pub fn remove(&mut self, key: &K) {
        let prefix_bytes = self.compute_prefix_for_key(key);
        let prefix = Keyspace::Context(&prefix_bytes);
        casper::remove(prefix).unwrap();
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let mut key_bytes = self.name.as_bytes().to_owned();
        key.serialize(&mut key_bytes).unwrap();
        let prefix = Keyspace::Context(&key_bytes);

        casper::read::<V>(prefix).unwrap()
    }

    fn compute_prefix_for_key(&self, key: &K) -> Vec<u8> {
        let mut context_key = Vec::new();
        context_key.extend(self.name.as_bytes());
        key.serialize(&mut context_key).unwrap();
        context_key
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<K: CasperABI, V: CasperABI> CasperABI for Map<K, V> {
    fn populate_definitions(definitions: &mut crate::abi::Definitions) {
        definitions.populate_one::<K>();
        definitions.populate_one::<V>();
    }

    fn declaration() -> Declaration {
        format!("Map<{}, {}>", K::declaration(), V::declaration())
    }

    #[inline]
    fn type_def() -> TypeDef {
        TypeDef::Struct {
            items: vec![StructField {
                name: "prefix".into(),
                decl: u64::declaration(),
            }],
        }
    }
}

impl<K: TypeUid, V: TypeUid> TypeUid for Map<K, V> {
    const UID: Uid = Uid::from_fields("Map", &[K::UID, V::UID, String::UID]);
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::casper::native::dispatch;

    use super::*;

    #[test]
    fn test_compute_prefix() {
        let prefix = compute_prefix("hello");
        assert_eq!(prefix.as_slice(), &[11, 189, 170, 128, 70, 216, 48, 164]);
        let back = u64::from_le_bytes(prefix);
        assert_eq!(fnv1a_hash_str_64("hello"), back);
    }

    #[test]
    fn test_map() {
        dispatch(|| {
            let mut map = Map::<u64, u64>::new("test");
            map.insert(&1, &2);
            assert_eq!(map.get(&1), Some(2));
            assert_eq!(map.get(&2), None);
            map.insert(&2, &3);
            assert_eq!(map.get(&1), Some(2));
            assert_eq!(map.get(&2), Some(3));

            let mut map = Map::<u64, u64>::new("test2");
            assert_eq!(map.get(&1), None);
            map.insert(&1, &22);
            assert_eq!(map.get(&1), Some(22));
            assert_eq!(map.get(&2), None);
            map.insert(&2, &33);
            assert_eq!(map.get(&1), Some(22));
            assert_eq!(map.get(&2), Some(33));
        })
        .unwrap();
    }
}
