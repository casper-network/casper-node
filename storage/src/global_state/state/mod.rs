//! Global state.

/// Lmdb implementation of global state.
pub mod lmdb;

/// Lmdb implementation of global state with cache.
pub mod scratch;

use std::{collections::HashMap, hash::BuildHasher};

use tracing::{debug, error, warn};

use casper_types::{bytesrepr, Digest, Key, StoredValue};

pub use self::lmdb::make_temporary_global_state;
use crate::global_state::{
    shared::{
        transform::{self, Transform, TransformInstruction},
        AdditiveMap,
    },
    transaction_source::{Transaction, TransactionSource},
    trie::{merkle_proof::TrieMerkleProof, Trie, TrieRaw},
    trie_store::{
        operations::{prune, read, write, ReadResult, WriteResult},
        TrieStore,
    },
};

use super::trie_store::operations::PruneResult;

/// A trait expressing the reading of state. This trait is used to abstract the underlying store.
pub trait StateReader<K, V> {
    /// An error which occurs when reading state
    type Error;

    /// Returns the state value from the corresponding key
    fn read(&self, key: &K) -> Result<Option<V>, Self::Error>;

    /// Returns the merkle proof of the state value from the corresponding key
    fn read_with_proof(&self, key: &K) -> Result<Option<TrieMerkleProof<K, V>>, Self::Error>;

    /// Returns the keys in the trie matching `prefix`.
    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<K>, Self::Error>;
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

    /// Reads a full `Trie` (never chunked) from the state if it is present
    fn get_trie_full(&self, trie_key: &Digest) -> Result<Option<TrieRaw>, Self::Error>;

    /// Insert a trie node into the trie
    fn put_trie(&self, trie: &[u8]) -> Result<Digest, Self::Error>;

    /// Finds all the children of `trie_raw` which aren't present in the state.
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, Self::Error>;

    /// Prunes key from the global state.
    fn prune_keys(&self, root: Digest, keys_to_delete: &[Key]) -> Result<PruneResult, Self::Error>;
}

/// Write multiple key/stored value pairs to the store in a single rw transaction.
pub fn put_stored_values<'a, R, S, E>(
    environment: &'a R,
    store: &S,
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
        let write_result = write::<_, _, _, _, E>(&mut txn, store, &state_root, key, value)?;
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
        let read_result = read::<_, _, _, _, E>(&txn, store, &state_root, &key)?;

        let instruction = match (read_result, transform) {
            (ReadResult::NotFound, Transform::Write(new_value)) => {
                TransformInstruction::store(new_value)
            }
            (ReadResult::NotFound, Transform::Prune(key)) => {
                // effectively a noop.
                debug!(
                    ?state_root,
                    ?key,
                    "commit: attempt to prune nonexistent record; this may happen if a key is both added and pruned in the same commit."
                );
                continue;
            }
            (ReadResult::NotFound, transform) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform,
                    "commit: key not found while attempting to apply transform"
                );
                return Err(CommitError::KeyNotFound(key).into());
            }
            (ReadResult::Found(current_value), transform) => match transform.apply(current_value) {
                Ok(instruction) => instruction,
                Err(err) => {
                    error!(
                        ?state_root,
                        ?key,
                        ?err,
                        "commit: key found, but could not apply transform"
                    );
                    return Err(CommitError::TransformError(err).into());
                }
            },
            (ReadResult::RootNotFound, transform) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform,
                    "commit: failed to read state root while processing transform"
                );
                return Err(CommitError::ReadRootNotFound(state_root).into());
            }
        };

        match instruction {
            TransformInstruction::Store(value) => {
                let write_result =
                    write::<_, _, _, _, E>(&mut txn, store, &state_root, &key, &value)?;

                match write_result {
                    WriteResult::Written(root_hash) => {
                        state_root = root_hash;
                    }
                    WriteResult::AlreadyExists => (),
                    WriteResult::RootNotFound => {
                        error!(?state_root, ?key, ?value, "commit: root not found");
                        return Err(CommitError::WriteRootNotFound(state_root).into());
                    }
                }
            }
            TransformInstruction::Prune(key) => {
                let prune_result = prune::<_, _, _, _, E>(&mut txn, store, &state_root, &key)?;

                match prune_result {
                    PruneResult::Pruned(root_hash) => {
                        state_root = root_hash;
                    }
                    PruneResult::DoesNotExist => {
                        warn!("commit: pruning attempt failed for {}", key);
                    }
                    PruneResult::RootNotFound => {
                        error!(?state_root, ?key, "commit: root not found");
                        return Err(CommitError::WriteRootNotFound(state_root).into());
                    }
                }
            }
        }
    }

    txn.commit()?;

    Ok(state_root)
}
