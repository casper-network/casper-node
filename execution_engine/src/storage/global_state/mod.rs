//! Global state.

/// In-memory implementation of global state.
pub mod in_memory;

/// Lmdb implementation of global state.
pub mod lmdb;

/// Lmdb implementation of global state with cache.
pub mod scratch;

use std::{collections::HashMap, hash::BuildHasher};

use tracing::error;

use casper_hashing::Digest;
use casper_types::{bytesrepr, bytesrepr::Bytes, Key, StoredValue};

use crate::{
    shared::{
        additive_map::AdditiveMap,
        newtypes::CorrelationId,
        transform::{self, Transform},
    },
    storage::{
        transaction_source::{Transaction, TransactionSource},
        trie::{merkle_proof::TrieMerkleProof, Trie, TrieOrChunk, TrieOrChunkId},
        trie_store::{
            operations::{read, write, ReadResult, WriteResult},
            TrieStore,
        },
    },
};

/// A trait expressing the reading of state. This trait is used to abstract the underlying store.
pub trait StateReader<K, V> {
    /// An error which occurs when reading state
    type Error;

    /// Returns the state value from the corresponding key
    fn read(&self, correlation_id: CorrelationId, key: &K) -> Result<Option<V>, Self::Error>;

    /// Returns the merkle proof of the state value from the corresponding key
    fn read_with_proof(
        &self,
        correlation_id: CorrelationId,
        key: &K,
    ) -> Result<Option<TrieMerkleProof<K, V>>, Self::Error>;

    /// Returns the keys in the trie matching `prefix`.
    fn keys_with_prefix(
        &self,
        correlation_id: CorrelationId,
        prefix: &[u8],
    ) -> Result<Vec<K>, Self::Error>;
}

/// An error emitted by the execution engine on commit
#[derive(Clone, Debug, thiserror::Error, Eq, PartialEq)]
pub enum CommitError {
    /// Root not found.
    #[error("Root not found: {0:?}")]
    RootNotFound(Digest),
    /// Root not found while attempting to read.
    #[error("Root not found while attempting to read: {0:?}")]
    ReadRootNotFound(Digest),
    /// Root not found while attempting to write.
    #[error("Root not found while writing: {0:?}")]
    WriteRootNotFound(Digest),
    /// Key not found.
    #[error("Key not found: {0}")]
    KeyNotFound(Key),
    /// Transform error.
    #[error(transparent)]
    TransformError(transform::Error),
    /// Trie not found while attempting to validate cache write.
    #[error("Trie not found in cache {0}")]
    TrieNotFoundInCache(Digest),
}

/// Provides `commit` method.
pub trait CommitProvider: StateProvider {
    /// Applies changes and returns a new post state hash.
    /// block_hash is used for computing a deterministic and unique keys.
    fn commit(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Self::Error>;
}

/// A trait expressing operations over the trie.
pub trait StateProvider {
    /// Associated error type for `StateProvider`.
    type Error;

    /// Associated reader type for `StateProvider`.
    type Reader: StateReader<Key, StoredValue, Error = Self::Error>;

    /// Checkouts to the post state of a specific block.
    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, Self::Error>;

    /// Returns an empty root hash.
    fn empty_root(&self) -> Digest;

    /// Reads a `Trie` (possibly chunked) from the state if it is present
    fn get_trie(
        &self,
        correlation_id: CorrelationId,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, Self::Error>;

    /// Reads a full `Trie` (never chunked) from the state if it is present
    fn get_trie_full(
        &self,
        correlation_id: CorrelationId,
        trie_key: &Digest,
    ) -> Result<Option<Bytes>, Self::Error>;

    /// Insert a trie node into the trie
    fn put_trie(&self, correlation_id: CorrelationId, trie: &[u8]) -> Result<Digest, Self::Error>;

    /// Finds all of the missing or corrupt keys of which are descendants of `trie_key`.
    fn missing_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_keys: Vec<Digest>,
    ) -> Result<Vec<Digest>, Self::Error>;
}

/// Write multiple key/stored value pairs to the store in a single rw transaction.
pub fn put_stored_values<'a, R, S, E>(
    environment: &'a R,
    store: &S,
    correlation_id: CorrelationId,
    prestate_hash: Digest,
    stored_values: HashMap<Key, StoredValue>,
) -> Result<Digest, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<Key, StoredValue>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<CommitError>,
{
    let mut txn = environment.create_read_write_txn()?;
    let mut state_root = prestate_hash;
    let maybe_root: Option<Trie<Key, StoredValue>> = store.get(&txn, &state_root)?;
    if maybe_root.is_none() {
        return Err(CommitError::RootNotFound(prestate_hash).into());
    };
    for (key, value) in stored_values.iter() {
        let write_result =
            write::<_, _, _, _, E>(correlation_id, &mut txn, store, &state_root, key, value)?;
        match write_result {
            WriteResult::Written(root_hash) => {
                state_root = root_hash;
            }
            WriteResult::AlreadyExists => (),
            WriteResult::RootNotFound => {
                error!(?state_root, ?key, ?value, "Error writing new value");
                return Err(CommitError::WriteRootNotFound(state_root).into());
            }
        }
    }
    txn.commit()?;
    Ok(state_root)
}

/// Commit `effects` to the store.
pub fn commit<'a, R, S, H, E>(
    environment: &'a R,
    store: &S,
    correlation_id: CorrelationId,
    prestate_hash: Digest,
    effects: AdditiveMap<Key, Transform, H>,
) -> Result<Digest, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<Key, StoredValue>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<CommitError>,
    H: BuildHasher,
{
    let mut txn = environment.create_read_write_txn()?;
    let mut state_root = prestate_hash;

    let maybe_root: Option<Trie<Key, StoredValue>> = store.get(&txn, &state_root)?;

    if maybe_root.is_none() {
        return Err(CommitError::RootNotFound(prestate_hash).into());
    };

    for (key, transform) in effects.into_iter() {
        let read_result = read::<_, _, _, _, E>(correlation_id, &txn, store, &state_root, &key)?;

        let value = match (read_result, transform) {
            (ReadResult::NotFound, Transform::Write(new_value)) => new_value,
            (ReadResult::NotFound, transform) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform,
                    "Key not found while attempting to apply transform"
                );
                return Err(CommitError::KeyNotFound(key).into());
            }
            (ReadResult::Found(current_value), transform) => match transform.apply(current_value) {
                Ok(updated_value) => updated_value,
                Err(err) => {
                    error!(
                        ?state_root,
                        ?key,
                        ?err,
                        "Key found, but could not apply transform"
                    );
                    return Err(CommitError::TransformError(err).into());
                }
            },
            (ReadResult::RootNotFound, transform) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform,
                    "Failed to read state root while processing transform"
                );
                return Err(CommitError::ReadRootNotFound(state_root).into());
            }
        };

        let write_result =
            write::<_, _, _, _, E>(correlation_id, &mut txn, store, &state_root, &key, &value)?;

        match write_result {
            WriteResult::Written(root_hash) => {
                state_root = root_hash;
            }
            WriteResult::AlreadyExists => (),
            WriteResult::RootNotFound => {
                error!(?state_root, ?key, ?value, "Error writing new value");
                return Err(CommitError::WriteRootNotFound(state_root).into());
            }
        }
    }

    txn.commit()?;

    Ok(state_root)
}
