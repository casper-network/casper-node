use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use lmdb::{
    self, Database, Environment, EnvironmentFlags, RoTransaction, RwTransaction, WriteFlags,
};
use rocksdb::{
    BoundColumnFamily, DBIteratorWithThreadMode, DBWithThreadMode, IteratorMode, MultiThreaded,
    Options,
};

use crate::storage::{
    error,
    transaction_source::{
        Readable, Transaction, TransactionSource, Writable,
        ROCKS_DB_LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY, ROCKS_DB_TRIE_V1_COLUMN_FAMILY,
    },
    trie_store::db::{ScratchCache, ScratchTrieStore},
    MAX_DBS,
};
use casper_types::bytesrepr::Bytes;

/// Filename for the LMDB database created by the EE.
const EE_LMDB_FILENAME: &str = "data.lmdb";
/// newtype over alt db.
#[derive(Clone)]
pub struct RocksDb {
    db: Arc<DBWithThreadMode<MultiThreaded>>,
}

/// Represents the state of a migration of a state root from lmdb to rocksdb.
pub enum RootMigration {
    /// Has the migration been left incomplete
    NotMigrated,
    /// Has the migration been not yet been started or completed
    Incomplete,
    /// Has the migration been completed
    Complete,
}

impl RootMigration {
    /// Has the migration been left incomplete
    pub fn is_incomplete(&self) -> bool {
        matches!(self, RootMigration::Incomplete)
    }
    /// Has the migration been not yet been started or completed
    pub fn is_not_migrated(&self) -> bool {
        matches!(self, RootMigration::NotMigrated)
    }
    /// Has the migration been completed
    pub fn is_complete(&self) -> bool {
        matches!(self, RootMigration::Complete)
    }
}

impl RocksDb {
    /// Check if a state root has been marked as migrated from lmdb to rocksdb.
    pub(crate) fn get_root_migration_state(
        &self,
        state_root: &[u8],
    ) -> Result<RootMigration, error::Error> {
        let lmdb_migration_column = self.lmdb_tries_migrated_column()?;
        let migration_state = self.db.get_cf(&lmdb_migration_column, state_root)?;
        Ok(match migration_state {
            Some(state) if state.is_empty() => RootMigration::Complete,
            Some(state) if state.get(0) == Some(&1u8) => RootMigration::Incomplete,
            _ => RootMigration::NotMigrated,
        })
    }

    /// Marks a state root as started migration from lmdb to rocksdb.
    pub(crate) fn mark_state_root_migration_incomplete(
        &self,
        state_root: &[u8],
    ) -> Result<(), error::Error> {
        let lmdb_migration_column = self.lmdb_tries_migrated_column()?;
        self.db.put_cf(&lmdb_migration_column, state_root, &[1u8])?;
        Ok(())
    }

    /// Marks a state root as fully migrated from lmdb to rocksdb.
    pub(crate) fn mark_state_root_migration_completed(
        &self,
        state_root: &[u8],
    ) -> Result<(), error::Error> {
        let lmdb_migration_column = self.lmdb_tries_migrated_column()?;
        self.db.put_cf(&lmdb_migration_column, state_root, &[])?;
        Ok(())
    }

    /// Trie V1 column family.
    fn trie_column_family(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db
            .cf_handle(ROCKS_DB_TRIE_V1_COLUMN_FAMILY)
            .ok_or_else(|| {
                error::Error::UnableToOpenColumnFamily(ROCKS_DB_TRIE_V1_COLUMN_FAMILY.to_string())
            })
    }

    /// Column family tracking state roots migrated from lmdb (supports safely resuming if the node
    /// were to be stopped during a migration).
    fn lmdb_tries_migrated_column(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db
            .cf_handle(ROCKS_DB_LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY)
            .ok_or_else(|| {
                error::Error::UnableToOpenColumnFamily(
                    ROCKS_DB_LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY.to_string(),
                )
            })
    }

    /// Trie store iterator.
    pub fn trie_store_iterator<'a: 'b, 'b>(
        &'a self,
    ) -> Result<DBIteratorWithThreadMode<'b, DBWithThreadMode<MultiThreaded>>, error::Error> {
        let cf_handle = self.trie_column_family()?;
        Ok(self.db.iterator_cf(&cf_handle, IteratorMode::Start))
    }
}

impl Transaction for RocksDb {
    type Error = error::Error;
    type Handle = RocksDb;
    fn commit(self) -> Result<(), Self::Error> {
        // NO OP as rocksdb doesn't use transactions.
        Ok(())
    }
}

impl Transaction for ScratchCache {
    type Error = error::Error;
    type Handle = ScratchCache;
    fn commit(self) -> Result<(), Self::Error> {
        // NO OP as scratch doesn't use transactions.
        Ok(())
    }
}

impl Readable for ScratchCache {
    fn read(&self, _handle: Self::Handle, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        let cf = self.rocksdb_store.rocksdb.trie_column_family()?;
        Ok(self.rocksdb_store.rocksdb.db.get_cf(&cf, key)?.map(|some| {
            let value = some.as_ref();
            Bytes::from(value)
        }))
    }
}

impl Writable for ScratchCache {
    fn write(
        &mut self,
        _handle: Self::Handle,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), Self::Error> {
        let cf = self.rocksdb_store.rocksdb.trie_column_family()?;
        let _result = self.rocksdb_store.rocksdb.db.put_cf(&cf, key, value)?;
        Ok(())
    }
}

impl Readable for RocksDb {
    fn read(&self, _handle: Self::Handle, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        let cf = self.trie_column_family()?;
        Ok(self.db.get_cf(&cf, key)?.map(|some| {
            let value = some.as_ref();
            Bytes::from(value)
        }))
    }
}

impl Writable for RocksDb {
    fn write(
        &mut self,
        _handle: Self::Handle,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), Self::Error> {
        let cf = self.trie_column_family()?;
        let _result = self.db.put_cf(&cf, key, value)?;
        Ok(())
    }
}

/// Environment for rocksdb.
#[derive(Clone)]
pub struct RocksDbStore {
    pub(crate) rocksdb: RocksDb,
    pub(crate) path: PathBuf,
}

impl RocksDbStore {
    /// Create a new environment for RocksDB.
    pub fn new(
        path: impl AsRef<Path>,
        rocksdb_opts: Options,
    ) -> Result<RocksDbStore, rocksdb::Error> {
        let db = Arc::new(DBWithThreadMode::<MultiThreaded>::open_cf(
            &rocksdb_opts,
            path.as_ref(),
            vec![
                ROCKS_DB_TRIE_V1_COLUMN_FAMILY,
                ROCKS_DB_LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY,
            ],
        )?);

        let rocksdb = RocksDb { db };

        Ok(RocksDbStore {
            rocksdb,
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Return the path to the backing rocksdb files.
    pub(crate) fn path(&self) -> PathBuf {
        self.path.clone()
    }

    /// Gets a scratch trie store.
    pub fn get_scratch_store(&self) -> ScratchTrieStore {
        ScratchTrieStore::new(self.clone())
    }
}

// TODO: remove this abstraction entirely when we've moved from lmdb to rocksdb for global state.
impl<'a> TransactionSource<'a> for ScratchTrieStore {
    type Error = error::Error;
    type Handle = ScratchCache;
    type ReadTransaction = ScratchCache;
    type ReadWriteTransaction = ScratchCache;
    fn create_read_txn(&'a self) -> Result<Self::ReadTransaction, Self::Error> {
        Ok(self.store.clone())
    }

    fn create_read_write_txn(&'a self) -> Result<Self::ReadWriteTransaction, Self::Error> {
        Ok(self.store.clone())
    }
}

impl<'a> TransactionSource<'a> for RocksDbStore {
    type Error = error::Error;
    type Handle = RocksDb;
    type ReadTransaction = RocksDb;
    type ReadWriteTransaction = RocksDb;
    fn create_read_txn(&'a self) -> Result<Self::ReadTransaction, Self::Error> {
        Ok(self.rocksdb.clone())
    }
    fn create_read_write_txn(&'a self) -> Result<Self::ReadWriteTransaction, Self::Error> {
        Ok(self.rocksdb.clone())
    }
}

impl<'a> Transaction for RoTransaction<'a> {
    type Error = lmdb::Error;

    type Handle = Database;

    fn commit(self) -> Result<(), Self::Error> {
        lmdb::Transaction::commit(self)
    }
}

impl<'a> Readable for RoTransaction<'a> {
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        match lmdb::Transaction::get(self, handle, &key) {
            Ok(bytes) => Ok(Some(Bytes::from(bytes))),
            Err(lmdb::Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl<'a> Transaction for RwTransaction<'a> {
    type Error = lmdb::Error;

    type Handle = Database;

    fn commit(self) -> Result<(), Self::Error> {
        <RwTransaction<'a> as lmdb::Transaction>::commit(self)
    }
}

impl<'a> Readable for RwTransaction<'a> {
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        match lmdb::Transaction::get(self, handle, &key) {
            Ok(bytes) => Ok(Some(Bytes::from(bytes))),
            Err(lmdb::Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl<'a> Writable for RwTransaction<'a> {
    fn write(&mut self, handle: Self::Handle, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        self.put(handle, &key, &value, WriteFlags::empty())
            .map_err(Into::into)
    }
}

/// The environment for an LMDB-backed trie store.
///
/// Wraps [`lmdb::Environment`].
#[derive(Debug)]
pub struct LmdbEnvironment {
    env: Environment,
    manual_sync_enabled: bool,
}

impl LmdbEnvironment {
    /// Constructor for `LmdbEnvironment`.
    pub fn new<P: AsRef<Path>>(
        path: P,
        map_size: usize,
        max_readers: u32,
        manual_sync_enabled: bool,
    ) -> Result<Self, error::Error> {
        let lmdb_flags = if manual_sync_enabled {
            // These options require that we manually call sync on the environment for the EE.
            EnvironmentFlags::NO_SUB_DIR
                | EnvironmentFlags::NO_READAHEAD
                | EnvironmentFlags::MAP_ASYNC
                | EnvironmentFlags::WRITE_MAP
                | EnvironmentFlags::NO_META_SYNC
        } else {
            EnvironmentFlags::NO_SUB_DIR | EnvironmentFlags::NO_READAHEAD
        };

        let env = Environment::new()
            // Set the flag to manage our own directory like in the storage component.
            .set_flags(lmdb_flags)
            .set_max_dbs(MAX_DBS)
            .set_map_size(map_size)
            .set_max_readers(max_readers)
            .open(&path.as_ref().join(EE_LMDB_FILENAME))?;
        Ok(LmdbEnvironment {
            env,
            manual_sync_enabled,
        })
    }

    /// Returns a reference to the wrapped `Environment`.
    pub fn env(&self) -> &Environment {
        &self.env
    }

    /// Returns if this environment was constructed with manual synchronization enabled.
    pub fn is_manual_sync_enabled(&self) -> bool {
        self.manual_sync_enabled
    }

    /// Manually synchronize LMDB to disk.
    pub fn sync(&self) -> Result<(), lmdb::Error> {
        self.env.sync(true)
    }
}

impl<'a> TransactionSource<'a> for LmdbEnvironment {
    type Error = lmdb::Error;
    type Handle = Database;
    type ReadTransaction = RoTransaction<'a>;
    type ReadWriteTransaction = RwTransaction<'a>;
    fn create_read_txn(&'a self) -> Result<RoTransaction<'a>, Self::Error> {
        self.env.begin_ro_txn()
    }
    fn create_read_write_txn(&'a self) -> Result<RwTransaction<'a>, Self::Error> {
        self.env.begin_rw_txn()
    }
}
