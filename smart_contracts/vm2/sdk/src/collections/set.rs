use crate::{
    abi::{CasperABI, Declaration, Definition, Definitions, StructField},
    prelude::marker::PhantomData,
};

use crate::{
    casper,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};
use casper_executor_wasm_common::{error::HostResult, keyspace::Keyspace};

use super::lookup_key::{Identity, LookupKey, LookupKeyOwned};

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct Set<T, L = Identity>
where
    T: BorshSerialize,
{
    prefix: String,
    lookup: L,
    _marker: PhantomData<T>,
}

impl<T: CasperABI + BorshSerialize, L> CasperABI for Set<T, L> {
    fn populate_definitions(_definitions: &mut Definitions) {}

    fn declaration() -> Declaration {
        format!("Set<{}>", T::declaration())
    }

    fn definition() -> Definition {
        Definition::Struct {
            items: vec![
                StructField {
                    name: "prefix".into(),
                    decl: String::declaration(),
                },
                StructField {
                    name: "length".into(),
                    decl: u64::declaration(),
                },
            ],
        }
    }
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
        casper::write(Keyspace::Context(lookup_key.as_ref()), &[]).unwrap();
    }

    pub fn contains(&self, key: &T) -> bool {
        let lookup_key = self.lookup.lookup(self.prefix.as_bytes(), key);
        let entry = casper::read(Keyspace::Context(lookup_key.as_ref()), |_size| None).unwrap();
        entry.is_some()
    }

    pub fn remove(&mut self, key: &T) -> bool {
        let lookup_key = self.lookup.lookup(self.prefix.as_bytes(), key);
        match casper::remove(Keyspace::Context(lookup_key.as_ref())) {
            Ok(()) => true,
            Err(HostResult::NotFound) => false,
            Err(other) => panic!("Error removing from set: {:?}", other),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{casper::native::dispatch, prelude::*};

    use crate::serializers::borsh::BorshSerialize;

    use super::Set;

    #[derive(BorshSerialize)]
    #[borsh(crate = "crate::serializers::borsh")]
    pub enum Flag {
        A,
        B,
        C,
    }

    #[test]
    fn should_insert() {
        dispatch(|| {
            let mut set: Set<Flag> = Set::new("Prefix".to_string());

            assert!(!set.contains(&Flag::A));
            assert!(!set.contains(&Flag::B));
            assert!(!set.contains(&Flag::C));

            set.insert(Flag::A);
            assert!(set.contains(&Flag::A));

            set.insert(Flag::B);
            assert!(set.contains(&Flag::B));

            set.insert(Flag::C);
            assert!(set.contains(&Flag::C));
        })
        .unwrap();
    }
}
