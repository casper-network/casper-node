pub mod in_memory;
pub mod lmdb;

use std::{fmt, hash::BuildHasher};

use crate::shared::{
    additive_map::AdditiveMap,
    newtypes::{Blake2bHash, CorrelationId},
    stored_value::StoredValue,
    transform::{self, Transform},
    TypeMismatch,
};
use casper_types::{bytesrepr, Key, ProtocolVersion};

use crate::storage::{
    protocol_data::ProtocolData,
    transaction_source::{Transaction, TransactionSource},
    trie::{merkle_proof::TrieMerkleProof, Trie},
    trie_store::{
        operations::{delete, read, write, DeleteResult, ReadResult, WriteResult},
        TrieStore,
    },
};

/// A reader of state
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

#[derive(Debug)]
pub enum CommitResult {
    RootNotFound,
    Success { state_root: Blake2bHash },
    KeyNotFound(Key),
    TypeMismatch(TypeMismatch),
    Serialization(bytesrepr::Error),
}

impl fmt::Display for CommitResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            CommitResult::RootNotFound => write!(f, "Root not found"),
            CommitResult::Success { state_root } => {
                write!(f, "Success: state_root: {}", state_root,)
            }
            CommitResult::KeyNotFound(key) => write!(f, "Key not found: {}", key),
            CommitResult::TypeMismatch(type_mismatch) => {
                write!(f, "Type mismatch: {:?}", type_mismatch)
            }
            CommitResult::Serialization(error) => write!(f, "Serialization: {:?}", error),
        }
    }
}

impl From<transform::Error> for CommitResult {
    fn from(error: transform::Error) -> Self {
        match error {
            transform::Error::TypeMismatch(type_mismatch) => {
                CommitResult::TypeMismatch(type_mismatch)
            }
            transform::Error::Serialization(error) => CommitResult::Serialization(error),
        }
    }
}

pub trait StateProvider {
    type Error;
    type Reader: StateReader<Key, StoredValue, Error = Self::Error>;

    /// Checkouts to the post state of a specific block.
    fn checkout(&self, state_hash: Blake2bHash) -> Result<Option<Self::Reader>, Self::Error>;

    /// Applies changes and returns a new post state hash.
    /// block_hash is used for computing a deterministic and unique keys.
    fn commit(
        &self,
        correlation_id: CorrelationId,
        state_hash: Blake2bHash,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<CommitResult, Self::Error>;

    fn put_protocol_data(
        &self,
        protocol_version: ProtocolVersion,
        protocol_data: &ProtocolData,
    ) -> Result<(), Self::Error>;

    fn get_protocol_data(
        &self,
        protocol_version: ProtocolVersion,
    ) -> Result<Option<ProtocolData>, Self::Error>;

    fn empty_root(&self) -> Blake2bHash;

    /// Reads a `Trie` from the state if it is present
    fn read_trie(
        &self,
        correlation_id: CorrelationId,
        trie_key: &Blake2bHash,
    ) -> Result<Option<Trie<Key, StoredValue>>, Self::Error>;

    /// Insert a trie node into the trie
    fn put_trie(
        &self,
        correlation_id: CorrelationId,
        trie: &Trie<Key, StoredValue>,
    ) -> Result<Blake2bHash, Self::Error>;

    /// Finds all of the missing or corrupt keys of which are descendants of `trie_key`
    fn missing_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_keys: Vec<Blake2bHash>,
    ) -> Result<Vec<Blake2bHash>, Self::Error>;
}

pub enum ModifyResult {
    WriteResult(WriteResult),
    DeleteResult(DeleteResult),
}

impl ModifyResult {
    pub fn updated_root(&self) -> Option<Blake2bHash> {
        match self {
            ModifyResult::WriteResult(WriteResult::Written(root_hash)) => Some(*root_hash),
            ModifyResult::DeleteResult(DeleteResult::Deleted(root_hash)) => Some(*root_hash),
            _ => None,
        }
    }
}

impl From<WriteResult> for ModifyResult {
    fn from(write_result: WriteResult) -> Self {
        ModifyResult::WriteResult(write_result)
    }
}

impl From<DeleteResult> for ModifyResult {
    fn from(delete_result: DeleteResult) -> Self {
        ModifyResult::DeleteResult(delete_result)
    }
}

pub fn commit<'a, R, S, H, E>(
    environment: &'a R,
    store: &S,
    correlation_id: CorrelationId,
    prestate_hash: Blake2bHash,
    effects: AdditiveMap<Key, Transform, H>,
) -> Result<CommitResult, E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<Key, StoredValue>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
    H: BuildHasher,
{
    let mut txn = environment.create_read_write_txn()?;
    let mut state_root = prestate_hash;

    let maybe_root: Option<Trie<Key, StoredValue>> = store.get(&txn, &state_root)?;

    if maybe_root.is_none() {
        return Ok(CommitResult::RootNotFound);
    }

    for (key, transform) in effects.into_iter() {
        let read_result = read::<_, _, _, _, E>(correlation_id, &txn, store, &state_root, &key)?;

        let modify_result: ModifyResult = match (read_result, transform) {
            // If we do not find a value, and the given transform is a write, then we should write
            // the value at the given key.
            (ReadResult::NotFound, Transform::Write(new_value)) => write::<_, _, _, _, E>(
                correlation_id,
                &mut txn,
                store,
                &state_root,
                &key,
                &new_value,
            )
            .map(Into::into)?,
            (ReadResult::NotFound, Transform::Delete) => {
                ModifyResult::DeleteResult(DeleteResult::DoesNotExist)
            }
            // If we do not find a value, and the given transform is a write, and the transform is
            // not a write, we should do something.
            (ReadResult::NotFound, _) => {
                return Ok(CommitResult::KeyNotFound(key));
            }
            // If we find a value, apply the given transform to it
            (ReadResult::Found(current_value), transform) => match transform.apply(current_value) {
                // A Some returned from Transform::Apply indicates that the value should be written
                // at the given key.
                Ok(Some(updated_value)) => write::<_, _, _, _, E>(
                    correlation_id,
                    &mut txn,
                    store,
                    &state_root,
                    &key,
                    &updated_value,
                )
                .map(Into::into)?,
                // A None returned from Transform::Apply indicates that the value under the given
                // key should be deleted.
                Ok(None) => {
                    delete::<_, _, _, _, E>(correlation_id, &mut txn, store, &state_root, &key)
                        .map(Into::into)?
                }
                Err(err) => return Ok(err.into()),
            },
            _x @ (ReadResult::RootNotFound, _) => panic!(stringify!(_x._1)),
        };

        match modify_result {
            ModifyResult::WriteResult(WriteResult::Written(root_hash)) => {
                state_root = root_hash;
            }
            ModifyResult::WriteResult(WriteResult::AlreadyExists) => (),
            ModifyResult::WriteResult(_x @ WriteResult::RootNotFound) => {
                panic!(stringify!(_x))
            }
            ModifyResult::DeleteResult(DeleteResult::Deleted(root_hash)) => {
                state_root = root_hash;
            }
            ModifyResult::DeleteResult(DeleteResult::DoesNotExist) => (),
            ModifyResult::DeleteResult(DeleteResult::RootNotFound) => {
                panic!(stringify!(_x))
            }
        }
    }

    txn.commit()?;

    Ok(CommitResult::Success { state_root })
}
