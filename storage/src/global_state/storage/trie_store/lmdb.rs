//! An LMDB-backed trie store.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use casper_types::bytesrepr;
use lmdb::{Database, DatabaseFlags, Transaction};

use casper_hashing::Digest;

use crate::global_state::storage::{
    error,
    state::CommitError,
    store::Store,
    transaction_source::{lmdb::LmdbEnvironment, Readable, TransactionSource, Writable},
    trie::Trie,
    trie_store::{self, TrieStore},
};

/// An LMDB-backed trie store.
///
/// Wraps [`lmdb::Database`].
#[derive(Debug, Clone)]
pub struct LmdbTrieStore {
    db: Database,
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

    /// Get a handle to the underlying database.
    pub fn get_db(&self) -> Database {
        self.db
    }
}

impl Store<Digest, Trie> for LmdbTrieStore {
    type Error = error::Error;

    type Handle = Database;

    fn handle(&self) -> Self::Handle {
        self.db
    }
}

impl TrieStore for LmdbTrieStore {}

/// Cache used by the scratch trie.  The keys represent the hash of the trie being cached.  The
/// values represent:  1) A boolean, where `false` means the trie was _not_ written and `true` means
/// it was 2) A deserialized trie
pub(crate) type Cache = Arc<Mutex<HashMap<Digest, (bool, Trie)>>>;

/// Cached version of the trie store.
#[derive(Clone)]
pub(crate) struct ScratchTrieStore {
    pub(crate) cache: Cache,
    pub(crate) store: Arc<LmdbTrieStore>,
    pub(crate) env: Arc<LmdbEnvironment>,
}

impl ScratchTrieStore {
    /// Creates a new ScratchTrieStore.
    pub fn new(store: Arc<LmdbTrieStore>, env: Arc<LmdbEnvironment>) -> Self {
        Self {
            store,
            env,
            cache: Default::default(),
        }
    }

    /// Writes only tries which are both under the given `state_root` and dirty to the underlying db
    /// while maintaining the invariant that children must be written before parent nodes.
    pub fn write_root_to_db(self, state_root: Digest) -> Result<(), error::Error> {
        let env = self.env;
        let store = self.store;
        let cache = &mut *self.cache.lock().map_err(|_| error::Error::Poison)?;

        let (is_root_dirty, root_trie) = cache
            .get(&state_root)
            .ok_or(CommitError::TrieNotFoundInCache(state_root))?;

        // Early exit if there is no work to do.
        if !is_root_dirty {
            return Ok(());
        }

        let mut txn = env.create_read_write_txn()?;
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
                store.put(&mut txn, &digest, current_trie)?;
            }
        }

        txn.commit()?;
        Ok(())
    }
}

impl Store<Digest, Trie> for ScratchTrieStore {
    type Error = error::Error;

    type Handle = ScratchTrieStore;

    fn handle(&self) -> Self::Handle {
        self.clone()
    }

    /// Puts a `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put<T>(&self, _txn: &mut T, digest: &Digest, trie: &Trie) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Self::Error: From<T::Error>,
    {
        self.cache
            .lock()
            .map_err(|_| error::Error::Poison)?
            .insert(*digest, (true, trie.clone()));
        Ok(())
    }

    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get<T>(&self, txn: &T, digest: &Digest) -> Result<Option<Trie>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Self::Error: From<T::Error>,
    {
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
                let raw = self.get_raw(txn, digest)?;
                match raw {
                    Some(bytes) => {
                        let value: Trie = bytesrepr::deserialize(bytes.into())?;
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

impl TrieStore for ScratchTrieStore {}
