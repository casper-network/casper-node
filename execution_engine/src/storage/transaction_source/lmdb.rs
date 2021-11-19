use std::{mem, path::Path};

use casper_types::bytesrepr::Bytes;
use lmdb::{
    self, Database, Environment, EnvironmentFlags, RoTransaction, RwTransaction, WriteFlags,
};
use tracing::debug;

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

// use lmdb::{Environment, Error};
use lmdb_sys::{self, MDB_env, MDB_SUCCESS};

/// Safe wrapper around environment info.
pub struct EnvInfo(lmdb_sys::MDB_envinfo);

impl EnvInfo {
    /// Returns last page number.
    pub fn last_pgno(&self) -> usize {
        self.0.me_last_pgno
    }

    /// Returns current map size.
    pub fn mapsize(&self) -> usize {
        self.0.me_mapsize
    }
}

fn lmdb_result(err_code: i32) -> Result<(), lmdb::Error> {
    if err_code == MDB_SUCCESS {
        Ok(())
    } else {
        Err(lmdb::Error::from_err_code(err_code))
    }
}

/// The environment for an LMDB-backed trie store.
///
/// Wraps [`lmdb::Environment`].
#[derive(Debug)]
pub struct LmdbEnvironment {
    env: Environment,
    manual_sync_enabled: bool,
    grow_size_threshold: usize,
    grow_size_bytes: usize,
}

impl LmdbEnvironment {
    /// Constructor for `LmdbEnvironment`.
    pub fn new<P: AsRef<Path>>(
        path: P,
        map_size: usize,
        max_readers: u32,
        manual_sync_enabled: bool,
        grow_size_threshold: usize,
        grow_size_bytes: usize,
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

        let lmdb_environment = LmdbEnvironment {
            env,
            manual_sync_enabled,
            grow_size_threshold,
            grow_size_bytes,
        };

        // Try increasing map size if the size exceeds the thresholds.
        lmdb_environment.resize_if_required()?;

        Ok(lmdb_environment)
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

    /// Get LMDB environment info.
    pub fn info(&self) -> Result<EnvInfo, lmdb::Error> {
        let env: *mut MDB_env = self.env().env();
        let mut env_info = mem::MaybeUninit::uninit();
        unsafe {
            lmdb_result(lmdb_sys::mdb_env_info(env, env_info.as_mut_ptr()))?;
            let env_info = EnvInfo(env_info.assume_init());
            Ok(env_info)
        }
    }

    fn set_map_size(&self, map_size: usize) -> Result<(), lmdb::Error> {
        let env: *mut MDB_env = self.env().env();
        let ret = unsafe { lmdb_sys::mdb_env_set_mapsize(env, map_size) };
        lmdb_result(ret)
    }

    fn resize_if_required(&self) -> Result<(), lmdb::Error> {
        let env_info = self.info()?;
        let stat = self.env.stat()?;
        let size_used_bytes = stat.page_size() as usize * env_info.last_pgno();
        let size_left_bytes = env_info.mapsize() - size_used_bytes;
        if size_left_bytes <= self.grow_size_threshold {
            let new_map_size = size_used_bytes + self.grow_size_bytes;
            self.set_map_size(new_map_size)?;
            debug!(
                self.grow_size_threshold,
                self.grow_size_bytes, size_used_bytes, new_map_size, "global state is resized"
            );
        }
        Ok(())
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

    fn create_read_write_txn(&'a self) -> Result<Self::ReadWriteTransaction, Self::Error> {
        self.resize_if_required()?;
        self.env.begin_rw_txn()
    }
}
