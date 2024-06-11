//! An LMDB-backed trie store.
//!
//! # Usage
//!
//! ```
//! use casper_storage::global_state::store::Store;
//! use casper_storage::global_state::transaction_source::{Transaction, TransactionSource};
//! use casper_storage::global_state::transaction_source::lmdb::LmdbEnvironment;
//! use casper_storage::global_state::trie::{PointerBlock, Trie};
//! use casper_storage::global_state::trie_store::lmdb::LmdbTrieStore;
//! use casper_types::Digest;
//! use casper_types::global_state::Pointer;
//! use casper_types::bytesrepr::{ToBytes, Bytes};
//! use lmdb::DatabaseFlags;
//! use tempfile::tempdir;
//!
//! // Create some leaves
//! let leaf_1 = Trie::Leaf { key: Bytes::from([0u8, 0, 0].as_slice()), value: Bytes::from(b"val_1".as_slice()) };
//! let leaf_2 = Trie::Leaf { key: Bytes::from([1u8, 0, 0].as_slice()), value: Bytes::from(b"val_2".as_slice()) };
//!
//! // Get their hashes
//! let leaf_1_hash = Digest::hash(&leaf_1.to_bytes().unwrap());
//! let leaf_2_hash = Digest::hash(&leaf_2.to_bytes().unwrap());
//!
//! // Create a node
//! let node: Trie<Bytes, Bytes> = {
//!     let mut pointer_block = PointerBlock::new();
//!     pointer_block[0] = Some(Pointer::LeafPointer(leaf_1_hash));
//!     pointer_block[1] = Some(Pointer::LeafPointer(leaf_2_hash));
//!     let pointer_block = Box::new(pointer_block);
//!     Trie::Node { pointer_block }
//! };
//!
//! // Get its hash
//! let node_hash = Digest::hash(&node.to_bytes().unwrap());
//!
//! // Create the environment and the store. For both the in-memory and
//! // LMDB-backed implementations, the environment is the source of
//! // transactions.
//! let tmp_dir = tempdir().unwrap();
//! let map_size = 4096 * 2560;  // map size should be a multiple of OS page size
//! let max_readers = 512;
//! let env = LmdbEnvironment::new(&tmp_dir.path().to_path_buf(), map_size, max_readers, true).unwrap();
//! let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();
//!
//! // First let's create a read-write transaction, persist the values, but
//! // forget to commit the transaction.
//! {
//!     // Create a read-write transaction
//!     let mut txn = env.create_read_write_txn().unwrap();
//!
//!     // Put the values in the store
//!     store.put(&mut txn, &leaf_1_hash, &leaf_1).unwrap();
//!     store.put(&mut txn, &leaf_2_hash, &leaf_2).unwrap();
//!     store.put(&mut txn, &node_hash, &node).unwrap();
//!
//!     // Here we forget to commit the transaction before it goes out of scope
//! }
//!
//! // Now let's check to see if the values were stored
//! {
//!     // Create a read transaction
//!     let txn = env.create_read_txn().unwrap();
//!
//!     // Observe that nothing has been persisted to the store
//!     for hash in [&leaf_1_hash, &leaf_2_hash, &node_hash].iter() {
//!         // We need to use a type annotation here to help the compiler choose
//!         // a suitable FromBytes instance
//!         let maybe_trie: Option<Trie<Bytes, Bytes>> = store.get(&txn, hash).unwrap();
//!         assert!(maybe_trie.is_none());
//!     }
//!
//!     // Commit the read transaction.  Not strictly necessary, but better to be hygienic.
//!     txn.commit().unwrap();
//! }
//!
//! // Now let's try that again, remembering to commit the transaction this time
//! {
//!     // Create a read-write transaction
//!     let mut txn = env.create_read_write_txn().unwrap();
//!
//!     // Put the values in the store
//!     store.put(&mut txn, &leaf_1_hash, &leaf_1).unwrap();
//!     store.put(&mut txn, &leaf_2_hash, &leaf_2).unwrap();
//!     store.put(&mut txn, &node_hash, &node).unwrap();
//!
//!     // Commit the transaction.
//!     txn.commit().unwrap();
//! }
//!
//! // Now let's check to see if the values were stored again
//! {
//!     // Create a read transaction
//!     let txn = env.create_read_txn().unwrap();
//!
//!     // Get the values in the store
//!     assert_eq!(Some(leaf_1), store.get(&txn, &leaf_1_hash).unwrap());
//!     assert_eq!(Some(leaf_2), store.get(&txn, &leaf_2_hash).unwrap());
//!     assert_eq!(Some(node), store.get(&txn, &node_hash).unwrap());
//!
//!     // Commit the read transaction.
//!     txn.commit().unwrap();
//! }
//!
//! tmp_dir.close().unwrap();
//! ```
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
    sync::{Arc, Mutex},
};

use lmdb::{Database, DatabaseFlags, Transaction};

use casper_types::{
    bytesrepr::{self, Bytes, ToBytes},
    Digest, Key, StoredValue,
};

use crate::global_state::{
    error,
    state::CommitError,
    store::Store,
    transaction_source::{lmdb::LmdbEnvironment, Readable, TransactionSource, Writable},
    trie::{LazilyDeserializedTrie, Trie},
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

impl<K, V> Store<Digest, Trie<K, V>> for LmdbTrieStore {
    type Error = error::Error;

    type Handle = Database;

    fn handle(&self) -> Self::Handle {
        self.db
    }
}

impl<K, V> TrieStore<K, V> for LmdbTrieStore {}

/// Cache used by the scratch trie.  The keys represent the hash of the trie being cached.  The
/// values represent:  1) A boolean, where `false` means the trie was _not_ written and `true` means
/// it was 2) A deserialized trie
pub(crate) type Cache = Arc<Mutex<HashMap<Digest, (bool, Bytes)>>>;

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

    /// Writes only tries which are both under the given `state_root` and dirty to the underlying
    /// db.
    pub fn write_root_to_db(self, state_root: Digest) -> Result<(), error::Error> {
        let cache = &*self.cache.lock().map_err(|_| error::Error::Poison)?;
        if !cache.contains_key(&state_root) {
            return Err(CommitError::TrieNotFoundInCache(state_root).into());
        }

        let mut tries_to_write = vec![state_root];
        let mut txn = self.env.create_read_write_txn()?;

        while let Some(trie_hash) = tries_to_write.pop() {
            let trie_bytes = if let Some((true, trie_bytes)) = cache.get(&trie_hash) {
                trie_bytes
            } else {
                // We don't have this trie in the scratch store or it's not dirty - do nothing.
                continue;
            };

            let lazy_trie: LazilyDeserializedTrie = bytesrepr::deserialize_from_slice(trie_bytes)?;
            tries_to_write.extend(lazy_trie.iter_children());

            Store::<Digest, Trie<Key, StoredValue>>::put_raw(
                &*self.store,
                &mut txn,
                &trie_hash,
                Cow::Borrowed(trie_bytes),
            )?;
        }

        txn.commit()?;
        Ok(())
    }
}

impl Store<Digest, Trie<Key, StoredValue>> for ScratchTrieStore {
    type Error = error::Error;

    type Handle = ScratchTrieStore;

    fn handle(&self) -> Self::Handle {
        self.clone()
    }

    fn get<T>(&self, txn: &T, key: &Digest) -> Result<Option<Trie<Key, StoredValue>>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Digest: ToBytes,
        Trie<Key, StoredValue>: bytesrepr::FromBytes,
        Self::Error: From<T::Error>,
    {
        match self.get_raw(txn, key)? {
            None => Ok(None),
            Some(value_bytes) => {
                let value = bytesrepr::deserialize(value_bytes.into())?;
                Ok(Some(value))
            }
        }
    }

    fn get_raw<T>(&self, txn: &T, key: &Digest) -> Result<Option<Bytes>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Self::Error: From<T::Error>,
    {
        let mut store = self.cache.lock().map_err(|_| error::Error::Poison)?;

        let maybe_trie = store.get(key);

        match maybe_trie {
            Some((_, trie_bytes)) => Ok(Some(trie_bytes.clone())),
            None => {
                let handle = self.handle();
                match txn.read(handle, key.as_ref())? {
                    Some(trie_bytes) => {
                        match store.entry(*key) {
                            Entry::Occupied(_) => {}
                            Entry::Vacant(v) => {
                                v.insert((false, trie_bytes.clone()));
                            }
                        }
                        Ok(Some(trie_bytes))
                    }
                    None => Ok(None),
                }
            }
        }
    }

    fn put<T>(
        &self,
        txn: &mut T,
        key: &Digest,
        value: &Trie<Key, StoredValue>,
    ) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Trie<Key, StoredValue>: ToBytes,
        Self::Error: From<T::Error>,
    {
        self.put_raw(txn, key, Cow::Owned(value.to_bytes()?))
    }

    fn put_raw<T>(
        &self,
        _txn: &mut T,
        key: &Digest,
        value_bytes: Cow<'_, [u8]>,
    ) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Self::Error: From<T::Error>,
    {
        self.cache
            .lock()
            .map_err(|_| error::Error::Poison)?
            .insert(*key, (true, Bytes::from(value_bytes.into_owned())));
        Ok(())
    }
}

impl TrieStore<Key, StoredValue> for ScratchTrieStore {}
