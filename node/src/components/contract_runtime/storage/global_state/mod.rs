pub mod in_memory;
pub mod lmdb;

use std::{collections::HashMap, fmt, hash::BuildHasher, time::Instant};

use crate::components::contract_runtime::shared::{
    additive_map::AdditiveMap,
    logging::{log_duration, log_metric},
    newtypes::{Blake2bHash, CorrelationId},
    stored_value::StoredValue,
    transform::{self, Transform},
    TypeMismatch,
};
use casperlabs_types::{account::AccountHash, bytesrepr, Key, ProtocolVersion, U512};

use crate::components::contract_runtime::storage::{
    protocol_data::ProtocolData,
    transaction_source::{Transaction, TransactionSource},
    trie::Trie,
    trie_store::{
        operations::{read, write, ReadResult, WriteResult},
        TrieStore,
    },
    GAUGE_METRIC_KEY,
};

const GLOBAL_STATE_COMMIT_READS: &str = "global_state_commit_reads";
const GLOBAL_STATE_COMMIT_WRITES: &str = "global_state_commit_writes";
const GLOBAL_STATE_COMMIT_DURATION: &str = "global_state_commit_duration";
const GLOBAL_STATE_COMMIT_READ_DURATION: &str = "global_state_commit_read_duration";
const GLOBAL_STATE_COMMIT_WRITE_DURATION: &str = "global_state_commit_write_duration";
const COMMIT: &str = "commit";

/// A reader of state
pub trait StateReader<K, V> {
    /// An error which occurs when reading state
    type Error;

    /// Returns the state value from the corresponding key
    fn read(&self, correlation_id: CorrelationId, key: &K) -> Result<Option<V>, Self::Error>;
}

#[derive(Debug)]
pub enum CommitResult {
    RootNotFound,
    Success {
        state_root: Blake2bHash,
        bonded_validators: HashMap<AccountHash, U512>,
    },
    KeyNotFound(Key),
    TypeMismatch(TypeMismatch),
    Serialization(bytesrepr::Error),
}

impl fmt::Display for CommitResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            CommitResult::RootNotFound => write!(f, "Root not found"),
            CommitResult::Success {
                state_root,
                bonded_validators,
            } => write!(
                f,
                "Success: state_root: {}, bonded_validators: {:?}",
                state_root, bonded_validators
            ),
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
    };

    let start = Instant::now();
    let mut reads: i32 = 0;
    let mut writes: i32 = 0;

    for (key, transform) in effects.into_iter() {
        let read_result = read::<_, _, _, _, E>(correlation_id, &txn, store, &state_root, &key)?;

        log_duration(
            correlation_id,
            GLOBAL_STATE_COMMIT_READ_DURATION,
            COMMIT,
            start.elapsed(),
        );

        reads += 1;

        let value = match (read_result, transform) {
            (ReadResult::NotFound, Transform::Write(new_value)) => new_value,
            (ReadResult::NotFound, _) => {
                return Ok(CommitResult::KeyNotFound(key));
            }
            (ReadResult::Found(current_value), transform) => match transform.apply(current_value) {
                Ok(updated_value) => updated_value,
                Err(err) => return Ok(err.into()),
            },
            _x @ (ReadResult::RootNotFound, _) => panic!(stringify!(_x._1)),
        };

        let write_result =
            write::<_, _, _, _, E>(correlation_id, &mut txn, store, &state_root, &key, &value)?;

        log_duration(
            correlation_id,
            GLOBAL_STATE_COMMIT_WRITE_DURATION,
            COMMIT,
            start.elapsed(),
        );

        match write_result {
            WriteResult::Written(root_hash) => {
                state_root = root_hash;
                writes += 1;
            }
            WriteResult::AlreadyExists => (),
            _x @ WriteResult::RootNotFound => panic!(stringify!(_x)),
        }
    }

    txn.commit()?;

    log_duration(
        correlation_id,
        GLOBAL_STATE_COMMIT_DURATION,
        COMMIT,
        start.elapsed(),
    );

    log_metric(
        correlation_id,
        GLOBAL_STATE_COMMIT_READS,
        COMMIT,
        GAUGE_METRIC_KEY,
        f64::from(reads),
    );

    log_metric(
        correlation_id,
        GLOBAL_STATE_COMMIT_WRITES,
        COMMIT,
        GAUGE_METRIC_KEY,
        f64::from(writes),
    );

    let bonded_validators = Default::default();

    Ok(CommitResult::Success {
        state_root,
        bonded_validators,
    })
}
