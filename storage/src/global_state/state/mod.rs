//! Global state.

/// Lmdb implementation of global state.
pub mod lmdb;

/// Lmdb implementation of global state with cache.
pub mod scratch;

use std::collections::HashMap;

use tracing::{debug, error, warn};

use casper_types::{
    bytesrepr,
    execution::{Effects, Transform, TransformError, TransformInstruction, TransformKind},
    Digest, Key, StoredValue,
};

#[cfg(test)]
pub use self::lmdb::make_temporary_global_state;

use crate::{
    data_access_layer::{BalanceRequest, BalanceResult, QueryRequest, QueryResult},
    global_state::{
        error::Error as GlobalStateError,
        transaction_source::{Transaction, TransactionSource},
        trie::{merkle_proof::TrieMerkleProof, Trie, TrieRaw},
        trie_store::{
            operations::{prune, read, write, ReadResult, WriteResult},
            TrieStore,
        },
    },
    system::{
        auction::bidding::{BiddingRequest, BiddingResult},
        mint::transfer::{TransferRequest, TransferResult},
    },
    tracking_copy::{TrackingCopy, TrackingCopyError, TrackingCopyExt},
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
    TransformError(TransformError),
    /// Trie not found while attempting to validate cache write.
    #[error("Trie not found in cache {0}")]
    TrieNotFoundInCache(Digest),
}

/// Provides `commit` method.
pub trait CommitProvider: StateProvider {
    /// Applies changes and returns a new post state hash.
    /// block_hash is used for computing a deterministic and unique keys.
    fn commit(&self, state_hash: Digest, effects: Effects) -> Result<Digest, GlobalStateError>;
}

/// A trait expressing operations over the trie.
pub trait StateProvider {
    /// Associated reader type for `StateProvider`.
    type Reader: StateReader<Key, StoredValue, Error = GlobalStateError>;

    /// Checkouts to the post state of a specific block.
    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, GlobalStateError>;

    /// Get a tracking copy.
    fn tracking_copy(
        &self,
        hash: Digest,
    ) -> Result<Option<TrackingCopy<Self::Reader>>, GlobalStateError>;

    /// Query state.
    fn query(&self, query_request: QueryRequest) -> QueryResult {
        match self.tracking_copy(query_request.state_hash()) {
            Ok(Some(tc)) => match tc.query(query_request.key(), query_request.path()) {
                Ok(ret) => ret.into(),
                Err(err) => QueryResult::TrackingCopyError(err),
            },
            Ok(None) => QueryResult::RootNotFound,
            Err(err) => QueryResult::StorageError(err),
        }
    }

    /// Returns an empty root hash.
    fn empty_root(&self) -> Digest;

    /// Reads a full `Trie` (never chunked) from the state if it is present
    fn get_trie_full(&self, trie_key: &Digest) -> Result<Option<TrieRaw>, GlobalStateError>;

    /// Insert a trie node into the trie
    fn put_trie(&self, trie: &[u8]) -> Result<Digest, GlobalStateError>;

    /// Finds all the children of `trie_raw` which aren't present in the state.
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, GlobalStateError>;

    /// Prunes key from the global state.
    fn prune_keys(
        &self,
        root: Digest,
        keys_to_delete: &[Key],
    ) -> Result<PruneResult, GlobalStateError>;

    /// Query balance state.
    fn balance(&self, balance_request: BalanceRequest) -> BalanceResult {
        match self.tracking_copy(balance_request.state_hash()) {
            Ok(Some(tracking_copy)) => {
                let purse_uref = balance_request.purse_uref();
                let purse_key = purse_uref.into();
                // read the new hold records if any exist
                // check their timestamps..if stale tc.prune(that item)
                // total bal - sum(hold balance) == avail
                match tracking_copy.get_purse_balance_key(purse_key) {
                    Ok(purse_balance_key) => {
                        match tracking_copy.get_purse_balance_with_proof(purse_balance_key) {
                            Ok((balance, proof)) => {
                                let proof = Box::new(proof);
                                let motes = balance.value();
                                BalanceResult::Success { motes, proof }
                            }
                            Err(err) => BalanceResult::Failure(err),
                        }
                    }
                    Err(err) => BalanceResult::Failure(err),
                }
            }
            Ok(None) => BalanceResult::RootNotFound,
            Err(err) => BalanceResult::Failure(TrackingCopyError::Storage(err)),
        }
    }

    fn transfer(&self, _transfer_request: TransferRequest) -> TransferResult {
        unimplemented!()
    }

    fn bidding(&self, _bid_request: BiddingRequest) -> BiddingResult {
        unimplemented!()
    }
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
pub fn commit<'a, R, S, E>(
    environment: &'a R,
    store: &S,
    prestate_hash: Digest,
    effects: Effects,
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

    for (key, kind) in effects.value().into_iter().map(Transform::destructure) {
        let read_result = read::<_, _, _, _, E>(&txn, store, &state_root, &key)?;

        let instruction = match (read_result, kind) {
            (_, TransformKind::Identity) => {
                // effectively a noop.
                debug!(?state_root, ?key, "commit: attempt to commit a read.");
                continue;
            }
            (ReadResult::NotFound, TransformKind::Write(new_value)) => {
                TransformInstruction::store(new_value)
            }
            (ReadResult::NotFound, TransformKind::Prune(key)) => {
                // effectively a noop.
                debug!(
                    ?state_root,
                    ?key,
                    "commit: attempt to prune nonexistent record; this may happen if a key is both added and pruned in the same commit."
                );
                continue;
            }
            (ReadResult::NotFound, transform_kind) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform_kind,
                    "commit: key not found while attempting to apply transform"
                );
                return Err(CommitError::KeyNotFound(key).into());
            }
            (ReadResult::Found(current_value), transform_kind) => {
                match transform_kind.apply(current_value) {
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
                }
            }
            (ReadResult::RootNotFound, transform_kind) => {
                error!(
                    ?state_root,
                    ?key,
                    ?transform_kind,
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
