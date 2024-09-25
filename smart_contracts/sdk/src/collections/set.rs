use crate::prelude::marker::PhantomData;

use crate::{host, serializers::borsh::BorshSerialize};
use casper_executor_wasm_common::keyspace::Keyspace;

use super::lookup_key::{Identity, LookupKey, LookupKeyOwned};

#[derive(Clone)]
pub struct Set<T, L = Identity>
where
    T: BorshSerialize,
{
    prefix: String,
    lookup: L,
    _marker: PhantomData<T>,
}

impl<T, L> Set<T, L>
where
    T: BorshSerialize,
    L: LookupKeyOwned,
    for<'a> <L as LookupKey<'a>>::Output: AsRef<[u8]>,
{
    pub fn new(prefix: String) -> Self {
        Self {
            prefix,
            lookup: L::default(),
            _marker: PhantomData,
        }
    }

    pub fn insert(&mut self, key: T) {
        let lookup_key = self.lookup.lookup(self.prefix.as_bytes(), &key);
        host::casper_write(Keyspace::Context(lookup_key.as_ref()), &[]).unwrap();
    }

    pub fn contains_key(&self, key: T) -> bool {
        let lookup_key = self.lookup.lookup(self.prefix.as_bytes(), &key);
        let entry =
            host::casper_read(Keyspace::Context(lookup_key.as_ref()), |_size| None).unwrap();
        entry.is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    use crate::serializers::borsh::BorshSerialize;

    use super::Set;

    #[derive(BorshSerialize)]
    #[borsh(crate = "crate::serializers::borsh")]
    pub enum Flag {
        A,
        B,
        C,
    }

    #[ignore]
    #[test]
    fn should_insert() {
        let mut set: Set<Flag> = Set::new("Prefix".to_string());

        assert!(!set.contains_key(Flag::A));
        assert!(!set.contains_key(Flag::B));
        assert!(!set.contains_key(Flag::C));

        set.insert(Flag::A);
        assert!(set.contains_key(Flag::A));

        set.insert(Flag::B);
        assert!(set.contains_key(Flag::B));

        set.insert(Flag::C);
        assert!(set.contains_key(Flag::C));
    }
}
