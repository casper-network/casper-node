use std::{ops::Deref, sync::Arc};

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{bytesrepr::Bytes, Key, StoredValue};

use crate::{
    shared::{additive_map::AdditiveMap, newtypes::CorrelationId, transform::Transform},
    storage::{
        error::{self, in_memory},
        global_state::{commit_effects, CommitProvider, StateProvider, StateReader},
        store::{BytesReader, Store},
        trie::{
            merkle_proof::TrieMerkleProof, operations::create_hashed_empty_trie, Trie, TrieOrChunk,
            TrieOrChunkId,
        },
        trie_store::{
            in_memory::InMemoryTrieStore,
            operations::{
                self, keys_with_prefix, missing_trie_keys, put_trie_bytes, read, read_with_proof,
                ReadResult, WriteResult,
            },
        },
    },
};

/// Global state implemented purely in memory only. No state is saved to disk. This is mostly
/// used for testing purposes.
pub struct InMemoryGlobalState {
    // Trie store for `InMemoryGlobalState`.
    pub(crate) trie_store: Arc<InMemoryTrieStore>,
    /// Empty state root hash.
    pub(crate) empty_root_hash: Digest,
}

/// Represents a "view" of global state at a particular root hash.
pub struct InMemoryGlobalStateView {
    /// Trie store for `InMemoryGlobalState`.
    pub(crate) store: Arc<InMemoryTrieStore>,
    /// State root hash for this "view".
    pub(crate) root_hash: Digest,
}

impl InMemoryGlobalState {
    /// Creates an empty state.
    pub fn empty() -> Result<Self, error::Error> {
        let trie_store = Arc::new(InMemoryTrieStore::new());
        let root_hash: Digest = {
            let (root_hash, root) = create_hashed_empty_trie::<Key, StoredValue>()?;
            trie_store.put(&root_hash, &root)?;
            root_hash
        };
        Ok(InMemoryGlobalState::new(trie_store, root_hash))
    }

    /// Creates a state from an existing environment, trie_Store, and root_hash.
    /// Intended to be used for testing.
    pub(crate) fn new(trie_store: Arc<InMemoryTrieStore>, empty_root_hash: Digest) -> Self {
        InMemoryGlobalState {
            trie_store,
            empty_root_hash,
        }
    }

    /// Creates a state from a given set of `Key, StoredValue` pairs.
    pub fn from_pairs(
        correlation_id: CorrelationId,
        pairs: &[(Key, StoredValue)],
    ) -> Result<(Self, Digest), error::Error> {
        let state = InMemoryGlobalState::empty()?;
        let mut current_root = state.empty_root_hash;
        {
            for (key, value) in pairs {
                let key = key.normalize();
                match operations::write::<Key, StoredValue, InMemoryTrieStore, in_memory::Error>(
                    correlation_id,
                    state.trie_store.deref(),
                    &current_root,
                    &key,
                    value,
                )? {
                    WriteResult::Written(root_hash) => {
                        current_root = root_hash;
                    }
                    WriteResult::AlreadyExists => (),
                    WriteResult::RootNotFound => panic!("InMemoryGlobalState has invalid root"),
                }
            }
        }
        Ok((state, current_root))
    }

    /// Returns the empty root hash owned by this `InMemoryGlobalState`.
    pub fn empty_root_hash(&self) -> Digest {
        self.empty_root_hash
    }
}

impl StateReader<Key, StoredValue> for InMemoryGlobalStateView {
    type Error = error::Error;

    fn read(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<StoredValue>, Self::Error> {
        let ret = match read::<Key, StoredValue, InMemoryTrieStore, Self::Error>(
            correlation_id,
            self.store.deref(),
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("InMemoryGlobalState has invalid root"),
        };
        Ok(ret)
    }

    fn read_with_proof(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        let ret = match read_with_proof::<Key, StoredValue, _, Self::Error>(
            correlation_id,
            self.store.deref(),
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("InMemoryGlobalState has invalid root"),
        };
        Ok(ret)
    }

    fn keys_with_prefix(
        &self,
        correlation_id: CorrelationId,
        prefix: &[u8],
    ) -> Result<Vec<Key>, Self::Error> {
        let keys_iter = keys_with_prefix::<Key, StoredValue, _>(
            correlation_id,
            self.store.deref(),
            &self.root_hash,
            prefix,
        );
        let mut ret: Vec<Key> = Vec::new();
        for result in keys_iter {
            match result {
                Ok(key) => ret.push(key),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(ret)
    }
}

impl CommitProvider for InMemoryGlobalState {
    fn commit_effects(
        &self,
        correlation_id: CorrelationId,
        prestate_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Self::Error> {
        commit_effects::<InMemoryTrieStore, _, Self::Error>(
            self.trie_store.deref(),
            correlation_id,
            prestate_hash,
            effects,
        )
        .map_err(Into::into)
    }
}

impl StateProvider for InMemoryGlobalState {
    type Error = error::Error;

    type Reader = InMemoryGlobalStateView;

    fn checkout(&self, prestate_hash: Digest) -> Result<Option<Self::Reader>, Self::Error> {
        let maybe_root: Option<Trie<Key, StoredValue>> = self.trie_store.get(&prestate_hash)?;
        let maybe_state = maybe_root.map(|_| InMemoryGlobalStateView {
            store: Arc::clone(&self.trie_store),
            root_hash: prestate_hash,
        });
        Ok(maybe_state)
    }

    fn empty_root(&self) -> Digest {
        self.empty_root_hash
    }

    fn get_trie_or_chunk(
        &self,
        _correlation_id: CorrelationId,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, Self::Error> {
        let TrieOrChunkId(trie_index, trie_key) = trie_or_chunk_id;
        let bytes = self.trie_store.read_bytes(trie_key.as_ref())?;
        bytes.map_or_else(
            || Ok(None),
            |bytes| {
                if bytes.len() <= ChunkWithProof::CHUNK_SIZE_BYTES {
                    Ok(Some(TrieOrChunk::Trie(bytes)))
                } else {
                    let chunk_with_proof = ChunkWithProof::new(&bytes, trie_index)?;
                    Ok(Some(TrieOrChunk::ChunkWithProof(chunk_with_proof)))
                }
            },
        )
    }

    fn get_trie_full(
        &self,
        _correlation_id: CorrelationId,
        trie_key: &Digest,
    ) -> Result<Option<Bytes>, Self::Error> {
        let ret: Option<Bytes> = self.trie_store.read_bytes(trie_key.as_ref())?;
        Ok(ret)
    }

    fn put_trie_bytes(
        &self,
        correlation_id: CorrelationId,
        trie: &[u8],
    ) -> Result<Digest, Self::Error> {
        let trie_hash = put_trie_bytes::<Key, StoredValue, InMemoryTrieStore, Self::Error>(
            correlation_id,
            self.trie_store.deref(),
            trie,
        )?;
        Ok(trie_hash)
    }

    /// Finds all of the keys of missing descendant `Trie<Key,StoredValue>` values.
    fn missing_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_keys: Vec<Digest>,
    ) -> Result<Vec<Digest>, Self::Error> {
        let missing_descendants =
            missing_trie_keys::<Key, StoredValue, InMemoryTrieStore, Self::Error>(
                correlation_id,
                self.trie_store.deref(),
                trie_keys,
                &Default::default(),
            )?;
        Ok(missing_descendants)
    }
}

#[cfg(test)]
mod tests {
    use casper_hashing::Digest;
    use casper_types::{account::AccountHash, CLValue};

    use super::*;

    #[derive(Debug, Clone)]
    struct TestPair {
        key: Key,
        value: StoredValue,
    }

    fn create_test_pairs() -> [TestPair; 2] {
        [
            TestPair {
                key: Key::Account(AccountHash::new([1_u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
            },
            TestPair {
                key: Key::Account(AccountHash::new([2_u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t(2_i32).unwrap()),
            },
        ]
    }

    fn create_test_pairs_updated() -> [TestPair; 3] {
        [
            TestPair {
                key: Key::Account(AccountHash::new([1u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t("one".to_string()).unwrap()),
            },
            TestPair {
                key: Key::Account(AccountHash::new([2u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t("two".to_string()).unwrap()),
            },
            TestPair {
                key: Key::Account(AccountHash::new([3u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t(3_i32).unwrap()),
            },
        ]
    }

    fn create_test_state() -> (InMemoryGlobalState, Digest) {
        InMemoryGlobalState::from_pairs(
            CorrelationId::new(),
            &create_test_pairs()
                .iter()
                .cloned()
                .map(|TestPair { key, value }| (key, value))
                .collect::<Vec<(Key, StoredValue)>>(),
        )
        .unwrap()
    }

    #[test]
    fn reads_from_a_checkout_return_expected_values() {
        let correlation_id = CorrelationId::new();
        let (state, root_hash) = create_test_state();
        let checkout = state.checkout(root_hash).unwrap().unwrap();
        for TestPair { key, value } in create_test_pairs().iter().cloned() {
            assert_eq!(Some(value), checkout.read(correlation_id, &key).unwrap());
        }
    }

    #[test]
    fn checkout_fails_if_unknown_hash_is_given() {
        let (state, _) = create_test_state();
        let fake_hash = Digest::hash(&[1, 2, 3]);
        let result = state.checkout(fake_hash).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn commit_updates_state() {
        let correlation_id = CorrelationId::new();

        let test_pairs_updated = create_test_pairs_updated();

        let (state, root_hash) = create_test_state();

        let effects: AdditiveMap<Key, Transform> = test_pairs_updated
            .iter()
            .cloned()
            .map(|TestPair { key, value }| (key, Transform::Write(value)))
            .collect();

        let updated_hash = state
            .commit_effects(correlation_id, root_hash, effects)
            .unwrap();

        let updated_checkout = state.checkout(updated_hash).unwrap().unwrap();

        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(
                Some(value),
                updated_checkout.read(correlation_id, &key).unwrap()
            );
        }
    }

    #[test]
    fn commit_updates_state_and_original_state_stays_intact() {
        let correlation_id = CorrelationId::new();
        let test_pairs_updated = create_test_pairs_updated();

        let (state, root_hash) = create_test_state();

        let effects: AdditiveMap<Key, Transform> = {
            let mut tmp = AdditiveMap::new();
            for TestPair { key, value } in &test_pairs_updated {
                tmp.insert(*key, Transform::Write(value.to_owned()));
            }
            tmp
        };

        let updated_hash = state
            .commit_effects(correlation_id, root_hash, effects)
            .unwrap();

        let updated_checkout = state.checkout(updated_hash).unwrap().unwrap();
        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(
                Some(value),
                updated_checkout.read(correlation_id, &key).unwrap()
            );
        }

        let original_checkout = state.checkout(root_hash).unwrap().unwrap();
        for TestPair { key, value } in create_test_pairs().iter().cloned() {
            assert_eq!(
                Some(value),
                original_checkout.read(correlation_id, &key).unwrap()
            );
        }
        assert_eq!(
            None,
            original_checkout
                .read(correlation_id, &test_pairs_updated[2].key)
                .unwrap()
        );
    }

    #[test]
    fn initial_state_has_the_expected_hash() {
        let correlation_id = CorrelationId::new();
        let expected_bytes = vec![
            197, 117, 38, 12, 241, 62, 54, 241, 121, 165, 11, 8, 130, 189, 100, 252, 4, 102, 236,
            210, 91, 221, 123, 200, 135, 102, 194, 204, 46, 76, 13, 254,
        ];
        let (_, root_hash) = InMemoryGlobalState::from_pairs(correlation_id, &[]).unwrap();
        assert_eq!(expected_bytes, root_hash.into_vec())
    }
}
