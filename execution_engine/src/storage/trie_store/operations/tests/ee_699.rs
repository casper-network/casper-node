use proptest::{arbitrary, array, collection, prop_oneof, strategy::Strategy};

use casper_hashing::Digest;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    gens, URef,
};

use super::{HashedTrie, TestValue};
use crate::{make_array_newtype, storage::trie::Trie};

pub const BASIC_LENGTH: usize = 4;
pub const SIMILAR_LENGTH: usize = 4;
pub const FANCY_LENGTH: usize = 5;
pub const LONG_LENGTH: usize = 8;

const PUBLIC_KEY_BASIC_ID: u8 = 0;
const PUBLIC_KEY_SIMILAR_ID: u8 = 1;
const PUBLIC_KEY_FANCY_ID: u8 = 2;
const PUBLIC_KEY_LONG_ID: u8 = 3;

pub const KEY_HASH_LENGTH: usize = 32;

const KEY_ACCOUNT_ID: u8 = 0;
const KEY_HASH_ID: u8 = 1;
const KEY_UREF_ID: u8 = 2;

make_array_newtype!(Basic, u8, BASIC_LENGTH);
make_array_newtype!(Similar, u8, SIMILAR_LENGTH);
make_array_newtype!(Fancy, u8, FANCY_LENGTH);
make_array_newtype!(Long, u8, LONG_LENGTH);

macro_rules! impl_distribution_for_array_newtype {
    ($name:ident, $ty:ty, $len:expr) => {
        impl rand::distributions::Distribution<$name> for rand::distributions::Standard {
            fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> $name {
                let mut dat = [0u8; $len];
                rng.fill_bytes(dat.as_mut());
                $name(dat)
            }
        }
    };
}

impl_distribution_for_array_newtype!(Basic, u8, BASIC_LENGTH);
impl_distribution_for_array_newtype!(Similar, u8, SIMILAR_LENGTH);
impl_distribution_for_array_newtype!(Fancy, u8, FANCY_LENGTH);
impl_distribution_for_array_newtype!(Long, u8, LONG_LENGTH);

macro_rules! make_array_newtype_arb {
    ($name:ident, $ty:ty, $len:expr, $fn_name:ident) => {
        fn $fn_name() -> impl Strategy<Value = $name> {
            collection::vec(arbitrary::any::<$ty>(), $len).prop_map(|values| {
                let mut dat = [0u8; $len];
                dat.copy_from_slice(values.as_slice());
                $name(dat)
            })
        }
    };
}

make_array_newtype_arb!(Basic, u8, BASIC_LENGTH, basic_arb);
make_array_newtype_arb!(Similar, u8, SIMILAR_LENGTH, similar_arb);
make_array_newtype_arb!(Fancy, u8, FANCY_LENGTH, fancy_arb);
make_array_newtype_arb!(Long, u8, LONG_LENGTH, long_arb);

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublicKey {
    Basic(Basic),
    Similar(Similar),
    Fancy(Fancy),
    Long(Long),
}

impl ToBytes for PublicKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::allocate_buffer(self)?;
        match self {
            PublicKey::Basic(key) => {
                ret.push(PUBLIC_KEY_BASIC_ID);
                ret.extend(key.to_bytes()?)
            }
            PublicKey::Similar(key) => {
                ret.push(PUBLIC_KEY_SIMILAR_ID);
                ret.extend(key.to_bytes()?)
            }
            PublicKey::Fancy(key) => {
                ret.push(PUBLIC_KEY_FANCY_ID);
                ret.extend(key.to_bytes()?)
            }
            PublicKey::Long(key) => {
                ret.push(PUBLIC_KEY_LONG_ID);
                ret.extend(key.to_bytes()?)
            }
        };
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                PublicKey::Basic(key) => key.serialized_length(),
                PublicKey::Similar(key) => key.serialized_length(),
                PublicKey::Fancy(key) => key.serialized_length(),
                PublicKey::Long(key) => key.serialized_length(),
            }
    }
}

impl FromBytes for PublicKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (id, rem): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match id {
            PUBLIC_KEY_BASIC_ID => {
                let (key, rem): (Basic, &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((PublicKey::Basic(key), rem))
            }
            PUBLIC_KEY_SIMILAR_ID => {
                let (key, rem): (Similar, &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((PublicKey::Similar(key), rem))
            }
            PUBLIC_KEY_FANCY_ID => {
                let (key, rem): (Fancy, &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((PublicKey::Fancy(key), rem))
            }
            PUBLIC_KEY_LONG_ID => {
                let (key, rem): (Long, &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((PublicKey::Long(key), rem))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

fn public_key_arb() -> impl Strategy<Value = PublicKey> {
    prop_oneof![
        basic_arb().prop_map(PublicKey::Basic),
        similar_arb().prop_map(PublicKey::Similar),
        fancy_arb().prop_map(PublicKey::Fancy),
        long_arb().prop_map(PublicKey::Long)
    ]
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TestKey {
    Account(PublicKey),
    Hash([u8; KEY_HASH_LENGTH]),
    URef(URef),
}

impl ToBytes for TestKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = Vec::with_capacity(self.serialized_length());
        match self {
            TestKey::Account(public_key) => {
                ret.push(KEY_ACCOUNT_ID);
                ret.extend(&public_key.to_bytes()?)
            }
            TestKey::Hash(hash) => {
                ret.push(KEY_HASH_ID);
                ret.extend(&hash.to_bytes()?)
            }
            TestKey::URef(uref) => {
                ret.push(KEY_UREF_ID);
                ret.extend(&uref.to_bytes()?)
            }
        }
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TestKey::Account(public_key) => public_key.serialized_length(),
                TestKey::Hash(hash) => hash.serialized_length(),
                TestKey::URef(uref) => uref.serialized_length(),
            }
    }
}

impl FromBytes for TestKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (id, rem): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match id {
            KEY_ACCOUNT_ID => {
                let (public_key, rem): (PublicKey, &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((TestKey::Account(public_key), rem))
            }
            KEY_HASH_ID => {
                let (hash, rem): ([u8; KEY_HASH_LENGTH], &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((TestKey::Hash(hash), rem))
            }
            KEY_UREF_ID => {
                let (uref, rem): (URef, &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((TestKey::URef(uref), rem))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

fn test_key_arb() -> impl Strategy<Value = TestKey> {
    prop_oneof![
        public_key_arb().prop_map(TestKey::Account),
        gens::u8_slice_32().prop_map(TestKey::Hash),
        gens::uref_arb().prop_map(TestKey::URef),
    ]
}

#[allow(clippy::unnecessary_operation)]
mod basics {
    use proptest::proptest;

    use super::*;

    #[test]
    fn random_key_generation_works_as_expected() {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let a: Basic = rng.gen();
        let b: Basic = rng.gen();
        assert_ne!(a, b)
    }

    proptest! {
        #[test]
        fn key_should_roundtrip(key in test_key_arb()) {
            bytesrepr::test_serialization_roundtrip(&key)
        }
    }
}

type TestTrie = Trie<TestKey, TestValue>;

const TEST_LEAVES_LENGTH: usize = 6;

/// Keys have been chosen deliberately and the `create_` functions below depend
/// on these exact definitions.  Values are arbitrary.
const TEST_LEAVES: [TestTrie; TEST_LEAVES_LENGTH] = [
    Trie::Leaf {
        key: TestKey::Account(PublicKey::Basic(Basic([0u8, 0, 0, 0]))),
        value: TestValue(*b"value0"),
    },
    Trie::Leaf {
        key: TestKey::Account(PublicKey::Basic(Basic([0u8, 0, 0, 1]))),
        value: TestValue(*b"value1"),
    },
    Trie::Leaf {
        key: TestKey::Account(PublicKey::Similar(Similar([0u8, 0, 0, 1]))),
        value: TestValue(*b"value3"),
    },
    Trie::Leaf {
        key: TestKey::Account(PublicKey::Fancy(Fancy([0u8, 0, 0, 1, 0]))),
        value: TestValue(*b"value4"),
    },
    Trie::Leaf {
        key: TestKey::Account(PublicKey::Long(Long([0u8, 0, 0, 1, 0, 0, 0, 0]))),
        value: TestValue(*b"value5"),
    },
    Trie::Leaf {
        key: TestKey::Hash([0u8; 32]),
        value: TestValue(*b"value6"),
    },
];

fn create_0_leaf_trie() -> Result<(Digest, Vec<HashedTrie<TestKey, TestValue>>), bytesrepr::Error> {
    let root = HashedTrie::new(Trie::node(&[]))?;

    let root_hash: Digest = root.hash;

    let parents: Vec<HashedTrie<TestKey, TestValue>> = vec![root];

    let tries: Vec<HashedTrie<TestKey, TestValue>> = {
        let mut ret = Vec::new();
        ret.extend(parents);
        ret
    };

    Ok((root_hash, tries))
}

mod empty_tries {
    use super::*;
    use crate::{
        shared::newtypes::CorrelationId,
        storage::{
            error,
            trie_store::operations::tests::{self, LmdbTestContext},
        },
    };

    #[test]
    fn lmdb_writes_to_n_leaf_empty_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let (root_hash, tries) = create_0_leaf_trie().unwrap();
        let context = LmdbTestContext::new(&tries).unwrap();
        let initial_states = vec![root_hash];

        let _states =
            tests::writes_to_n_leaf_empty_trie_had_expected_results::<_, _, _, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_states,
                &TEST_LEAVES,
            )
            .unwrap();
    }
}

mod proptests {
    use std::ops::RangeInclusive;

    use proptest::{collection::vec, proptest};

    use super::*;
    use crate::{
        shared::newtypes::CorrelationId,
        storage::{
            error::{self},
            trie_store::operations::tests::{self, LmdbTestContext},
        },
    };

    const DEFAULT_MIN_LENGTH: usize = 0;
    const DEFAULT_MAX_LENGTH: usize = 100;

    fn get_range() -> RangeInclusive<usize> {
        let start = option_env!("CL_TRIE_TEST_VECTOR_MIN_LENGTH")
            .and_then(|s| str::parse::<usize>(s).ok())
            .unwrap_or(DEFAULT_MIN_LENGTH);
        let end = option_env!("CL_TRIE_TEST_VECTOR_MAX_LENGTH")
            .and_then(|s| str::parse::<usize>(s).ok())
            .unwrap_or(DEFAULT_MAX_LENGTH);
        RangeInclusive::new(start, end)
    }

    fn lmdb_roundtrip_succeeds(pairs: &[(TestKey, TestValue)]) -> bool {
        let correlation_id = CorrelationId::new();
        let (root_hash, tries) = create_0_leaf_trie().unwrap();
        let context = LmdbTestContext::new(&tries).unwrap();
        let mut states_to_check = vec![];

        let root_hashes = tests::write_pairs::<_, _, _, _, error::Error>(
            correlation_id,
            &context.environment,
            &context.store,
            &root_hash,
            pairs,
        )
        .unwrap();

        states_to_check.extend(root_hashes);

        tests::check_pairs::<_, _, _, _, error::Error>(
            correlation_id,
            &context.environment,
            &context.store,
            &states_to_check,
            pairs,
        )
        .unwrap()
    }

    fn test_value_arb() -> impl Strategy<Value = TestValue> {
        array::uniform6(arbitrary::any::<u8>()).prop_map(TestValue)
    }

    proptest! {
        #[test]
        fn prop_lmdb_roundtrip_succeeds(inputs in vec((test_key_arb(), test_value_arb()), get_range())) {
            assert!(lmdb_roundtrip_succeeds(&inputs));
        }
    }
}
