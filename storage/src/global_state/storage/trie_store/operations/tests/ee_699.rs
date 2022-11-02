use casper_hashing::Digest;

use casper_types::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

use super::HashedTrie;
use crate::{global_state::storage::trie::Trie, make_array_newtype};

pub const BASIC_LENGTH: usize = 4;
pub const SIMILAR_LENGTH: usize = 4;
pub const FANCY_LENGTH: usize = 5;
pub const LONG_LENGTH: usize = 8;

const PUBLIC_KEY_BASIC_ID: u8 = 0;
const PUBLIC_KEY_SIMILAR_ID: u8 = 1;
const PUBLIC_KEY_FANCY_ID: u8 = 2;
const PUBLIC_KEY_LONG_ID: u8 = 3;

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

#[allow(clippy::unnecessary_operation)]
mod basics {

    use super::*;

    #[test]
    fn random_key_generation_works_as_expected() {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let a: Basic = rng.gen();
        let b: Basic = rng.gen();
        assert_ne!(a, b)
    }
}

fn create_0_leaf_trie() -> Result<(Digest, Vec<HashedTrie>), bytesrepr::Error> {
    let root = HashedTrie::new(Trie::node(&[]))?;
    let root_hash = root.hash;
    let parents = vec![root];
    let tries = {
        let mut ret = Vec::new();
        ret.extend(parents);
        ret
    };
    Ok((root_hash, tries))
}

mod empty_tries {
    use super::*;
    use crate::global_state::{
        shared::CorrelationId,
        storage::{
            error,
            trie_store::operations::tests::{self, LmdbTestContext, TEST_LEAVES},
        },
    };

    #[test]
    fn lmdb_writes_to_n_leaf_empty_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let (root_hash, tries) = create_0_leaf_trie().unwrap();
        let context = LmdbTestContext::new(&tries).unwrap();
        let initial_states = vec![root_hash];

        let _states =
            tests::writes_to_n_leaf_empty_trie_had_expected_results::<_, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_states,
                &*TEST_LEAVES,
            )
            .unwrap();
    }
}
