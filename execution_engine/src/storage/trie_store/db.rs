//! An LMDB-backed trie store.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use casper_types::{bytesrepr, Key, StoredValue};
use lmdb::{Database, DatabaseFlags};

use casper_hashing::Digest;

use crate::storage::{
    error,
    global_state::CommitError,
    store::Store,
    transaction_source::{
        db::{LmdbEnvironment, RocksDb, RocksDbStore},
        Readable, TransactionSource, Writable,
    },
    trie::Trie,
    trie_store::{self, TrieStore},
};

/// An LMDB-backed trie store.
///
/// Wraps [`lmdb::Database`].
#[derive(Debug, Clone)]
pub struct LmdbTrieStore {
    pub(crate) db: Database,
}

impl LmdbTrieStore {
    /// Constructor for new `LmdbTrieStore`.
    pub fn new(
        env: &LmdbEnvironment,
        maybe_name: Option<&str>,
        flags: DatabaseFlags,
    ) -> Result<Self, error::Error> {
        let name = Self::name(maybe_name);
        let db = env.env().create_db(Some(&name), flags)?;
        Ok(LmdbTrieStore { db })
    }

    /// Constructor for `LmdbTrieStore` which opens an existing lmdb store file.
    pub fn open(env: &LmdbEnvironment, maybe_name: Option<&str>) -> Result<Self, error::Error> {
        let name = Self::name(maybe_name);
        let db = env.env().open_db(Some(&name))?;
        Ok(LmdbTrieStore { db })
    }

    fn name(maybe_name: Option<&str>) -> String {
        maybe_name
            .map(|name| format!("{}-{}", trie_store::NAME, name))
            .unwrap_or_else(|| String::from(trie_store::NAME))
    }
}

impl<K, V> Store<Digest, Trie<K, V>> for LmdbTrieStore {
    type Error = error::Error;
    type Handle = Database;
    fn handle(&self) -> Self::Handle {
        self.db
    }
}

impl<K, V> TrieStore<K, V> for LmdbTrieStore {}

impl<K, V> Store<Digest, Trie<K, V>> for RocksDbStore {
    type Error = error::Error;
    type Handle = RocksDb;
    fn handle(&self) -> Self::Handle {
        self.rocksdb.clone()
    }
}

impl<K, V> TrieStore<K, V> for RocksDbStore {}

pub(crate) type Cache = Arc<Mutex<HashMap<Digest, (bool, Trie<Key, StoredValue>)>>>;
/// In-memory cached trie store, backed by rocksdb.
#[derive(Clone)]
pub struct ScratchCache {
    pub(crate) cache: Cache,
    pub(crate) rocksdb_store: RocksDbStore,
}

/// Cached version of the trie store.
#[derive(Clone)]
pub struct ScratchTrieStore {
    pub(crate) store: ScratchCache,
}

impl ScratchTrieStore {
    /// Creates a new ScratchTrieStore.
    pub fn new(rocksdb_store: RocksDbStore) -> Self {
        Self {
            store: ScratchCache {
                rocksdb_store,
                cache: Default::default(),
            },
        }
    }

    /// Writes all dirty (modified) cached tries to rocksdb.
    pub fn write_all_tries_to_rocksdb(self, new_state_root: Digest) -> Result<(), error::Error> {
        let rocksdb_store = self.store.rocksdb_store;
        let cache = &mut *self.store.cache.lock().map_err(|_| error::Error::Poison)?;

        let mut txn = rocksdb_store.create_read_write_txn()?;
        let mut missing_trie_keys = vec![new_state_root];
        let mut validated_tries = HashMap::new();

        while let Some(next_trie_key) = missing_trie_keys.pop() {
            if cache.is_empty() {
                return Err(error::Error::CommitError(
                    CommitError::TrieNotFoundDuringCacheValidate(next_trie_key),
                ));
            }
            match cache.remove(&next_trie_key) {
                Some((false, _)) => continue,
                None => {
                    let handle = rocksdb_store.rocksdb.clone();
                    txn.read(handle, next_trie_key.as_ref())?
                        .ok_or(error::Error::CommitError(
                            CommitError::TrieNotFoundDuringCacheValidate(next_trie_key),
                        ))?;
                }
                Some((true, trie)) => {
                    if let Some(children) = trie.children() {
                        missing_trie_keys.extend(children);
                    }
                    validated_tries.insert(next_trie_key, trie);
                }
            }
        }

        // after validating that all the needed tries are present, write everything
        for (digest, trie) in validated_tries.iter() {
            rocksdb_store.put(&mut txn, digest, trie)?;
        }

        Ok(())
    }
}

impl Store<Digest, Trie<Key, StoredValue>> for ScratchTrieStore {
    type Error = error::Error;

    type Handle = ScratchCache;

    fn handle(&self) -> Self::Handle {
        self.store.clone()
    }

    /// Puts a `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put<T>(
        &self,
        _txn: &mut T,
        digest: &Digest,
        trie: &Trie<Key, StoredValue>,
    ) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Self::Error: From<T::Error>,
    {
        self.store
            .cache
            .lock()
            .map_err(|_| error::Error::Poison)?
            .insert(*digest, (true, trie.clone()));
        Ok(())
    }

    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get<T>(
        &self,
        txn: &T,
        digest: &Digest,
    ) -> Result<Option<Trie<Key, StoredValue>>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Self::Error: From<T::Error>,
    {
        let maybe_trie = {
            self.store
                .cache
                .lock()
                .map_err(|_| error::Error::Poison)?
                .get(digest)
                .cloned()
        };
        match maybe_trie {
            Some((_, cached)) => Ok(Some(cached)),
            None => {
                let raw = self.get_raw(txn, digest)?;
                match raw {
                    Some(bytes) => {
                        let value: Trie<Key, StoredValue> =
                            bytesrepr::deserialize_from_slice(bytes)?;
                        {
                            let store =
                                &mut *self.store.cache.lock().map_err(|_| error::Error::Poison)?;
                            if !store.contains_key(digest) {
                                store.insert(*digest, (false, value.clone()));
                            }
                        }
                        Ok(Some(value))
                    }
                    None => Ok(None),
                }
            }
        }
    }
}

impl TrieStore<Key, StoredValue> for ScratchTrieStore {}
