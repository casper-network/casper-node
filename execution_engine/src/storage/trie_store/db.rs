//! A database-backed trie store.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use casper_types::{bytesrepr, bytesrepr::Bytes, Key, StoredValue};

use casper_hashing::Digest;
use rocksdb::{DBIteratorWithThreadMode, DBWithThreadMode, IteratorMode, MultiThreaded, Options};

use crate::storage::{
    db_store::DbStore,
    error,
    global_state::CommitError,
    store::{BytesReader, BytesWriter, ErrorSource, Store},
    trie::Trie,
    trie_store::TrieStore,
};

/// Cache used by the scratch trie.  The keys represent the hash of the trie being cached.  The
/// values represent:  1) A boolean, where `false` means the trie was _not_ written and `true` means
/// it was 2) A deserialized trie
pub(crate) type Cache = Arc<Mutex<HashMap<Digest, (bool, Trie<Key, StoredValue>)>>>;

/// Cached version of the trie store.
#[derive(Clone)]
pub(crate) struct ScratchTrieStore {
    pub(crate) cache: Cache,
    pub(crate) store: RocksDbTrieStore,
}

impl ScratchTrieStore {
    /// Creates a new ScratchTrieStore.
    pub fn new(store: RocksDbTrieStore) -> Self {
        Self {
            store,
            cache: Default::default(),
        }
    }

    /// Writes only tries which are both under the given `state_root` and dirty to the underlying db
    /// while maintaining the invariant that children must be written before parent nodes.
    pub fn write_root_to_db(self, state_root: Digest) -> Result<(), error::Error> {
        let store = self.store;
        let cache = &mut *self.cache.lock().map_err(|_| error::Error::Poison)?;

        let (is_root_dirty, root_trie) = cache
            .get(&state_root)
            .ok_or(CommitError::TrieNotFoundInCache(state_root))?;

        // Early exit if there is no work to do.
        if !is_root_dirty {
            return Ok(());
        }

        let mut tries_to_visit = vec![(state_root, root_trie, root_trie.iter_descendants())];

        while let Some((digest, current_trie, mut descendants_iterator)) = tries_to_visit.pop() {
            if let Some(descendant) = descendants_iterator.next() {
                tries_to_visit.push((digest, current_trie, descendants_iterator));
                // Only if a node is marked as dirty in the cache do we want to visit it's
                // descendants
                if let Some((true, child_trie)) = cache.get(&descendant) {
                    tries_to_visit.push((descendant, child_trie, child_trie.iter_descendants()));
                }
            } else {
                // We can write this node since it has no children, or they were already written.
                store.put(&digest, current_trie)?;
            }
        }

        Ok(())
    }
}

impl Store<Digest, Trie<Key, StoredValue>> for ScratchTrieStore {
    /// Puts a `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put(&self, digest: &Digest, trie: &Trie<Key, StoredValue>) -> Result<(), Self::Error> {
        self.cache
            .lock()
            .map_err(|_| error::Error::Poison)?
            .insert(*digest, (true, trie.clone()));
        Ok(())
    }

    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get(&self, digest: &Digest) -> Result<Option<Trie<Key, StoredValue>>, Self::Error> {
        let maybe_trie = {
            self.cache
                .lock()
                .map_err(|_| error::Error::Poison)?
                .get(digest)
                .cloned()
        };
        match maybe_trie {
            Some((_, cached)) => Ok(Some(cached)),
            None => {
                let raw = self.read_bytes(digest.as_ref())?;
                match raw {
                    Some(bytes) => {
                        let value: Trie<Key, StoredValue> = bytesrepr::deserialize(bytes.into())?;
                        {
                            let store =
                                &mut *self.cache.lock().map_err(|_| error::Error::Poison)?;
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

impl ErrorSource for ScratchTrieStore {
    type Error = error::Error;
}

impl BytesReader for ScratchTrieStore {
    fn read_bytes(&self, digest: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        self.store.read_bytes(digest)
    }
}

impl BytesWriter for ScratchTrieStore {
    fn write_bytes(&self, digest: &[u8], trie: &[u8]) -> Result<(), Self::Error> {
        self.store.write_bytes(digest, trie)
    }
}

impl ErrorSource for RocksDbTrieStore {
    type Error = error::Error;
}

impl BytesReader for RocksDbTrieStore {
    fn read_bytes(&self, digest: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        let cf = self.store.trie_column_family()?;
        Ok(self.store.db.get_cf(&cf, digest)?.map(|some| {
            let value = some.as_ref();
            Bytes::from(value)
        }))
    }
}

impl BytesWriter for RocksDbTrieStore {
    fn write_bytes(&self, digest: &[u8], trie: &[u8]) -> Result<(), Self::Error> {
        let cf = self.store.trie_column_family()?;
        let _result = self.store.db.put_cf(&cf, digest, trie)?;
        Ok(())
    }
}

/// Trie store.
#[derive(Clone)]
pub struct RocksDbTrieStore {
    store: DbStore,
}

/// Represents the state of a migration of a state root from lmdb to rocksdb.
pub enum RootMigration {
    /// Has the migration been not yet been started or completed
    NotStarted,
    /// Has the migration been left incomplete
    Partial,
    /// Has the migration been completed
    Complete,
}

impl RootMigration {
    /// Has the migration been not yet been started or completed
    pub fn is_not_started(&self) -> bool {
        matches!(self, RootMigration::NotStarted)
    }
    /// Has the migration been partially completed
    pub fn is_partial(&self) -> bool {
        matches!(self, RootMigration::Partial)
    }
    /// Has the migration been completed
    pub fn is_complete(&self) -> bool {
        matches!(self, RootMigration::Complete)
    }
}

impl RocksDbTrieStore {
    /// Create a new trie store backed by RocksDB.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, rocksdb::Error> {
        let store = DbStore::new(path)?;
        Ok(Self { store })
    }

    /// Create a new trie store for RocksDB with the specified options.
    pub fn new_with_opts(
        path: impl AsRef<Path>,
        rocksdb_opts: Options,
    ) -> Result<Self, rocksdb::Error> {
        let store = DbStore::new_with_opts(path, rocksdb_opts)?;
        Ok(Self { store })
    }

    /// Access the underlying DbStore.
    pub fn get_db_store(&self) -> &DbStore {
        &self.store
    }

    /// Check if a state root has been marked as migrated from lmdb to rocksdb.
    pub(crate) fn get_root_migration_state(
        &self,
        state_root: &[u8],
    ) -> Result<RootMigration, error::Error> {
        let lmdb_migration_column = self.store.lmdb_tries_migrated_column()?;
        let migration_state = self.store.db.get_cf(&lmdb_migration_column, state_root)?;
        Ok(match migration_state {
            Some(state) if state.is_empty() => RootMigration::Complete,
            Some(state) if state.get(0) == Some(&1u8) => RootMigration::Partial,
            _ => RootMigration::NotStarted,
        })
    }

    /// Marks a state root as started migration from lmdb to rocksdb.
    pub(crate) fn mark_state_root_migration_incomplete(
        &self,
        state_root: &[u8],
    ) -> Result<(), error::Error> {
        let lmdb_migration_column = self.store.lmdb_tries_migrated_column()?;
        self.store
            .db
            .put_cf(&lmdb_migration_column, state_root, &[1u8])?;
        Ok(())
    }

    /// Marks a state root as fully migrated from lmdb to rocksdb.
    pub(crate) fn mark_state_root_migration_completed(
        &self,
        state_root: &[u8],
    ) -> Result<(), error::Error> {
        let lmdb_migration_column = self.store.lmdb_tries_migrated_column()?;
        self.store
            .db
            .put_cf(&lmdb_migration_column, state_root, &[])?;
        Ok(())
    }

    /// Creates an iterator over all trie nodes.
    pub fn trie_store_iterator<'a: 'b, 'b>(
        &'a self,
    ) -> Result<DBIteratorWithThreadMode<'b, DBWithThreadMode<MultiThreaded>>, error::Error> {
        let cf_handle = self.store.trie_column_family()?;
        Ok(self.store.db.iterator_cf(&cf_handle, IteratorMode::Start))
    }
}

impl TrieStore<Key, StoredValue> for ScratchTrieStore {}

impl<K, V> Store<Digest, Trie<K, V>> for RocksDbTrieStore {}
impl<K, V> TrieStore<K, V> for RocksDbTrieStore {}
