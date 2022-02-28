use std::path::Path;

use lmdb::{
    self, Database, Environment, EnvironmentFlags, RoTransaction, RwTransaction, WriteFlags,
};

use crate::storage::{
    error,
    transaction_source::{Readable, Transaction, TransactionSource, Writable},
    MAX_DBS,
};

/// Filename for the LMDB database created by the EE.
const EE_DB_FILENAME: &str = "data.lmdb";

impl<'a> Transaction for RoTransaction<'a> {
    type Error = lmdb::Error;

    type Handle = Database;

    fn commit(self) -> Result<(), Self::Error> {
        lmdb::Transaction::commit(self)
    }
}

impl<'a> Readable for RoTransaction<'a> {
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<&[u8]>, Self::Error> {
        match lmdb::Transaction::get(self, handle, &key) {
            Ok(bytes) => Ok(Some(bytes)),
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
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<&[u8]>, Self::Error> {
        match lmdb::Transaction::get(self, handle, &key) {
            Ok(bytes) => Ok(Some(bytes)),
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
            .open(&path.as_ref().join(EE_DB_FILENAME))?;
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
