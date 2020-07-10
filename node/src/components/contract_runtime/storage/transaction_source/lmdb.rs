use std::path::Path;

use lmdb::{self, Database, Environment, RoTransaction, RwTransaction, WriteFlags};

use crate::components::contract_runtime::storage::{
    error,
    transaction_source::{Readable, Transaction, TransactionSource, Writable},
    MAX_DBS,
};

impl<'a> Transaction for RoTransaction<'a> {
    type Error = lmdb::Error;

    type Handle = Database;

    fn commit(self) -> Result<(), Self::Error> {
        lmdb::Transaction::commit(self)
    }
}

impl<'a> Readable for RoTransaction<'a> {
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        match lmdb::Transaction::get(self, handle, &key) {
            Ok(bytes) => Ok(Some(bytes.to_vec())),
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
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        match lmdb::Transaction::get(self, handle, &key) {
            Ok(bytes) => Ok(Some(bytes.to_vec())),
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
}

impl LmdbEnvironment {
    pub fn new<P: AsRef<Path>>(path: P, map_size: usize) -> Result<Self, error::Error> {
        let env = Environment::new()
            .set_max_dbs(MAX_DBS)
            .set_map_size(map_size)
            .open(path.as_ref())?;
        Ok(LmdbEnvironment { env })
    }

    pub fn env(&self) -> &Environment {
        &self.env
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
