//! This module defines the `TrackingCopy` - a utility that caches operations on the state, so that
//! the underlying state remains unmodified, but it can be interacted with as if the modifications
//! were applied on it.
mod byte_size;
mod ext;
pub(self) mod meter;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    convert::{From, TryInto},
};

use linked_hash_map::LinkedHashMap;
use thiserror::Error;

use casper_storage::global_state::{state::StateReader, trie::merkle_proof::TrieMerkleProof};
use casper_types::{
    addressable_entity::NamedKeys,
    bytesrepr::{self},
    execution::{Effects, Transform, TransformError, TransformInstruction, TransformKind},
    CLType, CLValue, CLValueError, Digest, Key, KeyTag, StoredValue, StoredValueTypeMismatch,
    Tagged, U512,
};

pub use self::ext::TrackingCopyExt;
use self::meter::{heap_meter::HeapSize, Meter};
use super::engine_state::EngineConfig;
use crate::runtime_context::dictionary;

/// Result of a query on a `TrackingCopy`.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum TrackingCopyQueryResult {
    /// The query was successful.
    Success {
        /// The value read from the state.
        value: StoredValue,
        /// Merkle proofs for the value.
        proofs: Vec<TrieMerkleProof<Key, StoredValue>>,
    },
    /// The value wasn't found.
    ValueNotFound(String),
    /// A circular reference was found in the state while traversing it.
    CircularReference(String),
    /// The query reached the depth limit.
    DepthLimit {
        /// The depth reached.
        depth: u64,
    },
}

/// Struct containing state relating to a given query.
struct Query {
    /// The key from where the search starts.
    base_key: Key,
    /// A collection of normalized keys which have been visited during the search.
    visited_keys: HashSet<Key>,
    /// The key currently being processed.
    current_key: Key,
    /// Path components which have not yet been followed, held in the same order in which they were
    /// provided to the `query()` call.
    unvisited_names: VecDeque<String>,
    /// Path components which have been followed, held in the same order in which they were
    /// provided to the `query()` call.
    visited_names: Vec<String>,
    /// Current depth of the query.
    depth: u64,
}

impl Query {
    fn new(base_key: Key, path: &[String]) -> Self {
        Query {
            base_key,
            current_key: base_key.normalize(),
            unvisited_names: path.iter().cloned().collect(),
            visited_names: Vec::new(),
            visited_keys: HashSet::new(),
            depth: 0,
        }
    }

    /// Panics if `unvisited_names` is empty.
    fn next_name(&mut self) -> &String {
        let next_name = self.unvisited_names.pop_front().unwrap();
        self.visited_names.push(next_name);
        self.visited_names.last().unwrap()
    }

    fn navigate(&mut self, key: Key) {
        self.current_key = key.normalize();
        self.depth += 1;
    }

    fn into_not_found_result(self, msg_prefix: &str) -> TrackingCopyQueryResult {
        let msg = format!("{} at path: {}", msg_prefix, self.current_path());
        TrackingCopyQueryResult::ValueNotFound(msg)
    }

    fn into_circular_ref_result(self) -> TrackingCopyQueryResult {
        let msg = format!(
            "{:?} has formed a circular reference at path: {}",
            self.current_key,
            self.current_path()
        );
        TrackingCopyQueryResult::CircularReference(msg)
    }

    fn into_depth_limit_result(self) -> TrackingCopyQueryResult {
        TrackingCopyQueryResult::DepthLimit { depth: self.depth }
    }

    fn current_path(&self) -> String {
        let mut path = format!("{:?}", self.base_key);
        for name in &self.visited_names {
            path.push('/');
            path.push_str(name);
        }
        path
    }
}

/// Keeps track of already accessed keys.
/// We deliberately separate cached Reads from cached mutations
/// because we want to invalidate Reads' cache so it doesn't grow too fast.
pub struct TrackingCopyCache<M> {
    max_cache_size: usize,
    current_cache_size: usize,
    reads_cached: LinkedHashMap<Key, StoredValue>,
    muts_cached: HashMap<Key, StoredValue>,
    prunes_cached: HashSet<Key>,
    key_tag_muts_cached: HashMap<KeyTag, BTreeSet<Key>>,
    meter: M,
}

impl<M: Meter<Key, StoredValue>> TrackingCopyCache<M> {
    /// Creates instance of `TrackingCopyCache` with specified `max_cache_size`,
    /// above which least-recently-used elements of the cache are invalidated.
    /// Measurements of elements' "size" is done with the usage of `Meter`
    /// instance.
    pub fn new(max_cache_size: usize, meter: M) -> TrackingCopyCache<M> {
        TrackingCopyCache {
            max_cache_size,
            current_cache_size: 0,
            reads_cached: LinkedHashMap::new(),
            muts_cached: HashMap::new(),
            key_tag_muts_cached: HashMap::new(),
            prunes_cached: HashSet::new(),
            meter,
        }
    }

    /// Inserts `key` and `value` pair to Read cache.
    pub fn insert_read(&mut self, key: Key, value: StoredValue) {
        let element_size = Meter::measure(&self.meter, &key, &value);
        self.reads_cached.insert(key, value);
        self.current_cache_size += element_size;
        while self.current_cache_size > self.max_cache_size {
            match self.reads_cached.pop_front() {
                Some((k, v)) => {
                    let element_size = Meter::measure(&self.meter, &k, &v);
                    self.current_cache_size -= element_size;
                }
                None => break,
            }
        }
    }

    /// Inserts `key` and `value` pair to Write/Add cache.
    pub fn insert_write(&mut self, key: Key, value: StoredValue) {
        self.prunes_cached.remove(&key);
        self.muts_cached.insert(key, value);

        let key_set = self
            .key_tag_muts_cached
            .entry(key.tag())
            .or_insert_with(BTreeSet::new);

        key_set.insert(key);
    }

    /// Inserts `key` and `value` pair to Write/Add cache.
    pub fn insert_prune(&mut self, key: Key) {
        self.prunes_cached.insert(key);
    }

    /// Gets value from `key` in the cache.
    pub fn get(&mut self, key: &Key) -> Option<&StoredValue> {
        if self.prunes_cached.get(key).is_some() {
            // the item is marked for pruning and therefore
            // is no longer accessible.
            return None;
        }
        if let Some(value) = self.muts_cached.get(key) {
            return Some(value);
        };

        self.reads_cached.get_refresh(key).map(|v| &*v)
    }

    /// Gets the set of mutated keys in the cache by `KeyTag`.
    pub fn get_key_tag_muts_cached(&mut self, key_tag: &KeyTag) -> Option<BTreeSet<Key>> {
        let pruned = &self.prunes_cached;
        if let Some(keys) = self.key_tag_muts_cached.get(key_tag) {
            let mut ret = BTreeSet::new();
            for key in keys {
                if !pruned.contains(key) {
                    ret.insert(*key);
                }
            }
            Some(ret)
        } else {
            None
        }
    }
}

/// An interface for the global state that caches all operations (reads and writes) instead of
/// applying them directly to the state. This way the state remains unmodified, while the user can
/// interact with it as if it was being modified in real time.
pub struct TrackingCopy<R> {
    reader: R,
    cache: TrackingCopyCache<HeapSize>,
    effects: Effects,
}

/// Result of executing an "add" operation on a value in the state.
#[derive(Debug)]
pub enum AddResult {
    /// The operation was successful.
    Success,
    /// The key was not found.
    KeyNotFound(Key),
    /// There was a type mismatch between the stored value and the value being added.
    TypeMismatch(StoredValueTypeMismatch),
    /// Serialization error.
    Serialization(bytesrepr::Error),
    /// Transform error.
    Transform(TransformError),
}

impl From<CLValueError> for AddResult {
    fn from(error: CLValueError) -> Self {
        match error {
            CLValueError::Serialization(error) => AddResult::Serialization(error),
            CLValueError::Type(type_mismatch) => {
                let expected = format!("{:?}", type_mismatch.expected);
                let found = format!("{:?}", type_mismatch.found);
                AddResult::TypeMismatch(StoredValueTypeMismatch::new(expected, found))
            }
        }
    }
}

impl<R: StateReader<Key, StoredValue>> TrackingCopy<R> {
    /// Creates a new `TrackingCopy` using the `reader` as the interface to the state.
    pub fn new(reader: R) -> TrackingCopy<R> {
        TrackingCopy {
            reader,
            // TODO: Should `max_cache_size` be a fraction of wasm memory limit?
            cache: TrackingCopyCache::new(1024 * 16, HeapSize),
            effects: Effects::new(),
        }
    }

    /// Returns the `reader` used to access the state.
    pub fn reader(&self) -> &R {
        &self.reader
    }

    /// Creates a new TrackingCopy, using this one (including its mutations) as
    /// the base state to read against. The intended use case for this
    /// function is to "snapshot" the current `TrackingCopy` and produce a
    /// new `TrackingCopy` where further changes can be made. This
    /// allows isolating a specific set of changes (those in the new
    /// `TrackingCopy`) from existing changes. Note that mutations to state
    /// caused by new changes (i.e. writes and adds) only impact the new
    /// `TrackingCopy`, not this one. Note that currently there is no `join` /
    /// `merge` function to bring changes from a fork back to the main
    /// `TrackingCopy`. this means the current usage requires repeated
    /// forking, however we recognize this is sub-optimal and will revisit
    /// in the future.
    pub fn fork(&self) -> TrackingCopy<&TrackingCopy<R>> {
        TrackingCopy::new(self)
    }

    pub(super) fn get(&mut self, key: &Key) -> Result<Option<StoredValue>, R::Error> {
        if let Some(value) = self.cache.get(key) {
            return Ok(Some(value.to_owned()));
        }
        if let Some(value) = self.reader.read(key)? {
            self.cache.insert_read(*key, value.to_owned());
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Gets the set of keys in the state whose tag is `key_tag`.
    pub fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, R::Error> {
        let mut ret: BTreeSet<Key> = BTreeSet::new();
        let keys = self.reader.keys_with_prefix(&[*key_tag as u8])?;
        let pruned = &self.cache.prunes_cached;
        // don't include keys marked for pruning
        for key in keys {
            if pruned.contains(&key) {
                continue;
            }
            ret.insert(key);
        }
        // there may be newly inserted keys which have not been committed yet
        if let Some(keys) = self.cache.get_key_tag_muts_cached(key_tag) {
            for key in keys {
                if ret.contains(&key) {
                    continue;
                }
                ret.insert(key);
            }
        }
        Ok(ret)
    }

    /// Reads the value stored under `key`.
    pub fn read(&mut self, key: &Key) -> Result<Option<StoredValue>, R::Error> {
        let normalized_key = key.normalize();
        if let Some(value) = self.get(&normalized_key)? {
            self.effects
                .push(Transform::new(normalized_key, TransformKind::Identity));
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Writes `value` under `key`. Note that the write is only cached, and the global state itself
    /// remains unmodified.
    pub fn write(&mut self, key: Key, value: StoredValue) {
        let normalized_key = key.normalize();
        self.cache.insert_write(normalized_key, value.clone());
        let transform = Transform::new(normalized_key, TransformKind::Write(value));
        self.effects.push(transform);
    }

    /// Prunes a `key`.
    pub(crate) fn prune(&mut self, key: Key) {
        let normalized_key = key.normalize();
        self.cache.insert_prune(normalized_key);
        self.effects
            .push(Transform::new(normalized_key, TransformKind::Prune(key)));
    }

    /// Ok(None) represents missing key to which we want to "add" some value.
    /// Ok(Some(unit)) represents successful operation.
    /// Err(error) is reserved for unexpected errors when accessing global
    /// state.
    pub fn add(&mut self, key: Key, value: StoredValue) -> Result<AddResult, R::Error> {
        let normalized_key = key.normalize();
        let current_value = match self.get(&normalized_key)? {
            None => return Ok(AddResult::KeyNotFound(normalized_key)),
            Some(current_value) => current_value,
        };

        let type_name = value.type_name();
        let mismatch = || {
            Ok(AddResult::TypeMismatch(StoredValueTypeMismatch::new(
                "I32, U64, U128, U256, U512 or (String, Key) tuple".to_string(),
                type_name,
            )))
        };

        let transform_kind = match value {
            StoredValue::CLValue(cl_value) => match *cl_value.cl_type() {
                CLType::I32 => match cl_value.into_t() {
                    Ok(value) => TransformKind::AddInt32(value),
                    Err(error) => return Ok(AddResult::from(error)),
                },
                CLType::U64 => match cl_value.into_t() {
                    Ok(value) => TransformKind::AddUInt64(value),
                    Err(error) => return Ok(AddResult::from(error)),
                },
                CLType::U128 => match cl_value.into_t() {
                    Ok(value) => TransformKind::AddUInt128(value),
                    Err(error) => return Ok(AddResult::from(error)),
                },
                CLType::U256 => match cl_value.into_t() {
                    Ok(value) => TransformKind::AddUInt256(value),
                    Err(error) => return Ok(AddResult::from(error)),
                },
                CLType::U512 => match cl_value.into_t() {
                    Ok(value) => TransformKind::AddUInt512(value),
                    Err(error) => return Ok(AddResult::from(error)),
                },
                _ => {
                    if *cl_value.cl_type() == casper_types::named_key_type() {
                        match cl_value.into_t() {
                            Ok((name, key)) => {
                                let mut named_keys = NamedKeys::new();
                                named_keys.insert(name, key);
                                TransformKind::AddKeys(named_keys)
                            }
                            Err(error) => return Ok(AddResult::from(error)),
                        }
                    } else {
                        return mismatch();
                    }
                }
            },
            _ => return mismatch(),
        };

        match transform_kind.clone().apply(current_value) {
            Ok(TransformInstruction::Store(new_value)) => {
                self.cache.insert_write(normalized_key, new_value);
                self.effects
                    .push(Transform::new(normalized_key, transform_kind));
                Ok(AddResult::Success)
            }
            Ok(TransformInstruction::Prune(key)) => {
                self.cache.insert_prune(normalized_key);
                self.effects
                    .push(Transform::new(normalized_key, TransformKind::Prune(key)));
                Ok(AddResult::Success)
            }
            Err(TransformError::TypeMismatch(type_mismatch)) => {
                Ok(AddResult::TypeMismatch(type_mismatch))
            }
            Err(TransformError::Serialization(error)) => Ok(AddResult::Serialization(error)),
            Err(transform_error) => Ok(AddResult::Transform(transform_error)),
        }
    }

    /// Returns a copy of the execution effects cached by this instance.
    pub fn effects(&self) -> Effects {
        self.effects.clone()
    }

    /// Calling `query()` avoids calling into `self.cache`, so this will not return any values
    /// written or mutated in this `TrackingCopy` via previous calls to `write()` or `add()`, since
    /// these updates are only held in `self.cache`.
    ///
    /// The intent is that `query()` is only used to satisfy `QueryRequest`s made to the server.
    /// Other EE internal use cases should call `read()` or `get()` in order to retrieve cached
    /// values.
    pub fn query(
        &self,
        config: &EngineConfig,
        base_key: Key,
        path: &[String],
    ) -> Result<TrackingCopyQueryResult, R::Error> {
        let mut query = Query::new(base_key, path);

        let mut proofs = Vec::new();

        loop {
            if query.depth >= config.max_query_depth {
                return Ok(query.into_depth_limit_result());
            }

            if !query.visited_keys.insert(query.current_key) {
                return Ok(query.into_circular_ref_result());
            }

            let stored_value = match self.reader.read_with_proof(&query.current_key)? {
                None => {
                    return Ok(query.into_not_found_result("Failed to find base key"));
                }
                Some(stored_value) => stored_value,
            };

            let value = stored_value.value().to_owned();

            // Following code does a patching on the `StoredValue` that unwraps an inner
            // `DictionaryValue` for dictionaries only.
            let value = match dictionary::handle_stored_value(query.current_key, value) {
                Ok(patched_stored_value) => patched_stored_value,
                Err(error) => {
                    return Ok(query.into_not_found_result(&format!(
                        "Failed to retrieve dictionary value: {}",
                        error
                    )))
                }
            };

            proofs.push(stored_value);

            if query.unvisited_names.is_empty() {
                return Ok(TrackingCopyQueryResult::Success { value, proofs });
            }

            let stored_value: &StoredValue = proofs
                .last()
                .map(|r| r.value())
                .expect("but we just pushed");

            match stored_value {
                StoredValue::Account(account) => {
                    let name = query.next_name();
                    if let Some(key) = account.named_keys().get(name) {
                        query.navigate(*key);
                    } else {
                        let msg_prefix = format!("Name {} not found in Account", name);
                        return Ok(query.into_not_found_result(&msg_prefix));
                    }
                }
                StoredValue::Contract(contract) => {
                    let name = query.next_name();
                    if let Some(key) = contract.named_keys().get(name) {
                        query.navigate(*key);
                    } else {
                        let msg_prefix = format!("Name {} not found in Contract", name);
                        return Ok(query.into_not_found_result(&msg_prefix));
                    }
                }
                StoredValue::CLValue(cl_value) if cl_value.cl_type() == &CLType::Key => {
                    if let Ok(key) = cl_value.to_owned().into_t::<Key>() {
                        query.navigate(key);
                    } else {
                        return Ok(query.into_not_found_result("Failed to parse CLValue as Key"));
                    }
                }
                StoredValue::CLValue(cl_value) => {
                    let msg_prefix = format!(
                        "Query cannot continue as {:?} is not an account, contract nor key to \
                        such.  Value found",
                        cl_value
                    );
                    return Ok(query.into_not_found_result(&msg_prefix));
                }
                StoredValue::AddressableEntity(entity) => {
                    let name = query.next_name();
                    if let Some(key) = entity.named_keys().get(name) {
                        query.navigate(*key);
                    } else {
                        let msg_prefix = format!("Name {} not found in Contract", name);
                        return Ok(query.into_not_found_result(&msg_prefix));
                    }
                }
                StoredValue::ContractPackage(_) => {
                    return Ok(query.into_not_found_result("ContractPackage value found."));
                }
                StoredValue::ContractWasm(_) => {
                    return Ok(query.into_not_found_result("ContractWasm value found."));
                }
                StoredValue::Transfer(_) => {
                    return Ok(query.into_not_found_result("Transfer value found."));
                }
                StoredValue::DeployInfo(_) => {
                    return Ok(query.into_not_found_result("DeployInfo value found."));
                }
                StoredValue::EraInfo(_) => {
                    return Ok(query.into_not_found_result("EraInfo value found."));
                }
                StoredValue::Bid(_) => {
                    return Ok(query.into_not_found_result("Bid value found."));
                }
                StoredValue::BidKind(_) => {
                    return Ok(query.into_not_found_result("BidKind value found."));
                }
                StoredValue::Withdraw(_) => {
                    return Ok(query.into_not_found_result("WithdrawPurses value found."));
                }
                StoredValue::Unbonding(_) => {
                    return Ok(query.into_not_found_result("UnbondingPurses value found."));
                }
                StoredValue::URef(_, _) => {
                    return Ok(query.into_not_found_result("URef value found."));
                }
            }
        }
    }
}

/// The purpose of this implementation is to allow a "snapshot" mechanism for
/// TrackingCopy. The state of a TrackingCopy (including the effects of
/// any transforms it has accumulated) can be read using an immutable
/// reference to that TrackingCopy via this trait implementation. See
/// `TrackingCopy::fork` for more information.
impl<R: StateReader<Key, StoredValue>> StateReader<Key, StoredValue> for &TrackingCopy<R> {
    type Error = R::Error;

    fn read(&self, key: &Key) -> Result<Option<StoredValue>, Self::Error> {
        if let Some(value) = self.cache.muts_cached.get(key) {
            return Ok(Some(value.to_owned()));
        }
        if let Some(value) = self.reader.read(key)? {
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn read_with_proof(
        &self,
        key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        self.reader.read_with_proof(key)
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Key>, Self::Error> {
        self.reader.keys_with_prefix(prefix)
    }
}

/// Error conditions of a proof validation.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// The path should not have a different length than the proof less one.
    #[error("The path should not have a different length than the proof less one.")]
    PathLengthDifferentThanProofLessOne,

    /// The provided key does not match the key in the proof.
    #[error("The provided key does not match the key in the proof.")]
    UnexpectedKey,

    /// The provided value does not match the value in the proof.
    #[error("The provided value does not match the value in the proof.")]
    UnexpectedValue,

    /// The proof hash is invalid.
    #[error("The proof hash is invalid.")]
    InvalidProofHash,

    /// The path went cold.
    #[error("The path went cold.")]
    PathCold,

    /// (De)serialization error.
    #[error("Serialization error: {0}")]
    BytesRepr(bytesrepr::Error),

    /// Key is not a URef.
    #[error("Key is not a URef")]
    KeyIsNotAURef(Key),

    /// Error converting a stored value to a [`Key`].
    #[error("Failed to convert stored value to key")]
    ValueToCLValueConversion,

    /// CLValue conversion error.
    #[error("{0}")]
    CLValueError(CLValueError),
}

impl From<CLValueError> for ValidationError {
    fn from(err: CLValueError) -> Self {
        ValidationError::CLValueError(err)
    }
}

impl From<bytesrepr::Error> for ValidationError {
    fn from(error: bytesrepr::Error) -> Self {
        Self::BytesRepr(error)
    }
}

/// Validates proof of the query.
///
/// Returns [`ValidationError`] for any of
pub fn validate_query_proof(
    hash: &Digest,
    proofs: &[TrieMerkleProof<Key, StoredValue>],
    expected_first_key: &Key,
    path: &[String],
    expected_value: &StoredValue,
) -> Result<(), ValidationError> {
    if proofs.len() != path.len() + 1 {
        return Err(ValidationError::PathLengthDifferentThanProofLessOne);
    }

    let mut proofs_iter = proofs.iter();
    let mut path_components_iter = path.iter();

    // length check above means we are safe to unwrap here
    let first_proof = proofs_iter.next().unwrap();

    if first_proof.key() != &expected_first_key.normalize() {
        return Err(ValidationError::UnexpectedKey);
    }

    if hash != &first_proof.compute_state_hash()? {
        return Err(ValidationError::InvalidProofHash);
    }

    let mut proof_value = first_proof.value();

    for proof in proofs_iter {
        let named_keys = match proof_value {
            StoredValue::Account(account) => account.named_keys(),
            StoredValue::Contract(contract) => contract.named_keys(),
            StoredValue::AddressableEntity(entity) => entity.named_keys(),
            StoredValue::CLValue(_) => {
                proof_value = proof.value();
                continue;
            }
            _ => return Err(ValidationError::PathCold),
        };

        let path_component = match path_components_iter.next() {
            Some(path_component) => path_component,
            None => return Err(ValidationError::PathCold),
        };

        let key = match named_keys.get(path_component) {
            Some(key) => key,
            None => return Err(ValidationError::PathCold),
        };

        if proof.key() != &key.normalize() {
            return Err(ValidationError::UnexpectedKey);
        }

        if hash != &proof.compute_state_hash()? {
            return Err(ValidationError::InvalidProofHash);
        }

        proof_value = proof.value();
    }

    if proof_value != expected_value {
        return Err(ValidationError::UnexpectedValue);
    }

    Ok(())
}

/// Validates a proof of a balance request.
pub fn validate_balance_proof(
    hash: &Digest,
    balance_proof: &TrieMerkleProof<Key, StoredValue>,
    expected_purse_key: Key,
    expected_motes: &U512,
) -> Result<(), ValidationError> {
    let expected_balance_key = expected_purse_key
        .into_uref()
        .map(|uref| Key::Balance(uref.addr()))
        .ok_or_else(|| ValidationError::KeyIsNotAURef(expected_purse_key.to_owned()))?;

    if balance_proof.key() != &expected_balance_key.normalize() {
        return Err(ValidationError::UnexpectedKey);
    }

    if hash != &balance_proof.compute_state_hash()? {
        return Err(ValidationError::InvalidProofHash);
    }

    let balance_proof_stored_value = balance_proof.value().to_owned();

    let balance_proof_clvalue: CLValue = balance_proof_stored_value
        .try_into()
        .map_err(|_| ValidationError::ValueToCLValueConversion)?;

    let balance_motes: U512 = balance_proof_clvalue.into_t()?;

    if expected_motes != &balance_motes {
        return Err(ValidationError::UnexpectedValue);
    }

    Ok(())
}
