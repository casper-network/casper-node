use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use lmdb::{self, Environment, EnvironmentFlags};
use rocksdb::{
    BoundColumnFamily, DBIteratorWithThreadMode, DBWithThreadMode, IteratorMode, MultiThreaded,
    Options,
};

use casper_types::bytesrepr::Bytes;

use crate::storage::{
    error,
    transaction_source::{
        Readable, Writable, LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY, TRIE_V1_COLUMN_FAMILY,
    },
    trie_store::db::ScratchTrieStore,
    MAX_DBS,
};

use super::{ErrorSource, WORKING_SET_COLUMN_FAMILY};

/// Filename for the LMDB database created by the EE.
const EE_LMDB_FILENAME: &str = "data.lmdb";

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

impl ErrorSource for ScratchTrieStore {
    type Error = error::Error;
}

impl Readable for ScratchTrieStore {
    fn read(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        self.store.read(key)
    }
}

impl Writable for ScratchTrieStore {
    fn write(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        self.store.write(key, value)
    }
}

impl ErrorSource for RocksDbStore {
    type Error = error::Error;
}

impl Readable for RocksDbStore {
    fn read(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        let cf = self.trie_column_family()?;
        Ok(self.db.get_cf(&cf, key)?.map(|some| {
            let value = some.as_ref();
            Bytes::from(value)
        }))
    }
}

impl Writable for RocksDbStore {
    fn write(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        let cf = self.trie_column_family()?;
        let _result = self.db.put_cf(&cf, key, value)?;
        Ok(())
    }
}

/// Environment for rocksdb.
#[derive(Clone)]
pub struct RocksDbStore {
    db: Arc<DBWithThreadMode<MultiThreaded>>,
    pub(crate) path: PathBuf,
}

impl RocksDbStore {
    /// Create a new environment for RocksDB.
    pub(crate) fn new(
        path: impl AsRef<Path>,
        rocksdb_opts: Options,
    ) -> Result<RocksDbStore, rocksdb::Error> {
        let db = Arc::new(DBWithThreadMode::<MultiThreaded>::open_cf(
            &rocksdb_opts,
            path.as_ref(),
            vec![
                TRIE_V1_COLUMN_FAMILY,
                LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY,
                WORKING_SET_COLUMN_FAMILY,
            ],
        )?);

        Ok(RocksDbStore {
            db,
            path: path.as_ref().to_path_buf(),
        })
    }
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
        self.db.cf_handle(TRIE_V1_COLUMN_FAMILY).ok_or_else(|| {
            error::Error::UnableToOpenColumnFamily(TRIE_V1_COLUMN_FAMILY.to_string())
        })
    }

    /// Column family tracking state roots migrated from lmdb (supports safely resuming if the node
    /// were to be stopped during a migration).
    fn lmdb_tries_migrated_column(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db
            .cf_handle(LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY)
            .ok_or_else(|| {
                error::Error::UnableToOpenColumnFamily(
                    LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY.to_string(),
                )
            })
    }

    /// Working set column family.
    pub fn working_set_column_family(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db.cf_handle(WORKING_SET_COLUMN_FAMILY).ok_or_else(|| {
            error::Error::UnableToOpenColumnFamily(WORKING_SET_COLUMN_FAMILY.to_string())
        })
    }

    /// Trie store iterator.
    pub fn trie_store_iterator<'a: 'b, 'b>(
        &'a self,
    ) -> Result<DBIteratorWithThreadMode<'b, DBWithThreadMode<MultiThreaded>>, error::Error> {
        let cf_handle = self.trie_column_family()?;
        Ok(self.db.iterator_cf(&cf_handle, IteratorMode::Start))
    }

    /// Return the path to the backing rocksdb files.
    pub(crate) fn path(&self) -> PathBuf {
        self.path.clone()
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
