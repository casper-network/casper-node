use crate::{
    abi::{CasperABI, Definition, StructField},
    host::{self, read_vec},
    storage::Keyspace,
};

use borsh::{self, BorshDeserialize, BorshSerialize};
use const_fnv1a_hash::fnv1a_hash_str_64;

use std::{iter, marker::PhantomData};

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Vector<T> {
    pub(crate) prefix: String,
    pub(crate) length: u64,
    pub(crate) _marker: PhantomData<T>,
}

/// Computes the prefix for a given key.
#[allow(dead_code)]
pub(crate) const fn compute_prefix(input: &str) -> [u8; 8] {
    let hash = fnv1a_hash_str_64(input);
    hash.to_le_bytes()
}

impl<T> Vector<T>
where
    T: BorshSerialize + BorshDeserialize,
{
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        Self {
            prefix: prefix.into(),
            length: 0,
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, value: T) {
        let mut prefix_bytes = self.prefix.as_bytes().to_owned();
        prefix_bytes.extend(&self.length.to_le_bytes());
        let prefix = Keyspace::Context(&prefix_bytes);
        host::casper_write(prefix, 0, &borsh::to_vec(&value).unwrap()).unwrap();
        self.length += 1;
    }

    pub fn get(&self, index: u64) -> Option<T> {
        let mut prefix_bytes = self.prefix.as_bytes().to_owned();
        prefix_bytes.extend(&index.to_le_bytes());
        let prefix = Keyspace::Context(&prefix_bytes);
        match read_vec(prefix) {
            Some(vec) => Some(borsh::from_slice(&vec).unwrap()),
            None => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.length).map(move |i| self.get(i).unwrap())
    }

    pub fn len(&self) -> u64 {
        self.length
    }
}

impl<T: CasperABI> CasperABI for Vector<T> {
    #[inline]
    fn definition() -> Definition {
        Definition::Struct {
            items: vec![
                StructField {
                    name: "prefix".into(),
                    body: String::definition(),
                },
                StructField {
                    name: "length".into(),
                    body: u64::definition(),
                },
            ],
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_vec() {
        let mut vec = Vector::<u64>::new("test");

        assert!(vec.get(0).is_none());
        vec.push(111);
        assert_eq!(vec.get(0), Some(111));
        vec.push(222);
        assert_eq!(vec.get(1), Some(222));

        let mut iter = vec.iter();
        assert_eq!(iter.next(), Some(111));
        assert_eq!(iter.next(), Some(222));
        assert_eq!(iter.next(), None);

        {
            let ser = borsh::to_vec(&vec).unwrap();
            let deser: Vector<u64> = borsh::from_slice(&ser).unwrap();
            let mut iter = deser.iter();
            assert_eq!(iter.next(), Some(111));
            assert_eq!(iter.next(), Some(222));
            assert_eq!(iter.next(), None);
        }

        let vec2 = Vector::<u64>::new("test1");
        assert_eq!(vec2.get(0), None);
    }
}
