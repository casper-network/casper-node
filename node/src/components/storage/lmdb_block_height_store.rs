use std::{
    fmt::Debug,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use lmdb::{
    self, Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction, WriteFlags,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::{BlockHeightStore, Error, Result};

/// LMDB version of a store.
#[derive(Debug)]
pub(super) struct LmdbBlockHeightStore {
    env: Environment,
    db: Database,
    highest: AtomicU64,
}

impl LmdbBlockHeightStore {
    pub(crate) fn new<P: AsRef<Path>>(db_path: P, max_size: usize) -> Result<Self> {
        let env = Environment::new()
            .set_flags(EnvironmentFlags::NO_SUB_DIR)
            .set_map_size(max_size)
            .open(db_path.as_ref())?;
        let db = env.create_db(None, DatabaseFlags::INTEGER_KEY)?;

        // Get the last key, which will represent the largest block height, since the LMDB is sorted
        // by integer key increasing.
        let mut max_height_bytes = [0; 8];
        let txn = env.begin_ro_txn().expect("should create ro txn");
        {
            let mut cursor = txn.open_ro_cursor(db).expect("should create ro cursor");
            for (height_bytes, _value) in cursor.iter() {
                max_height_bytes.copy_from_slice(height_bytes);
            }
        }
        txn.commit().expect("should commit txn");
        let highest = AtomicU64::new(u64::from_ne_bytes(max_height_bytes));

        info!("opened DB at {}", db_path.as_ref().display());

        Ok(LmdbBlockHeightStore { env, db, highest })
    }
}

impl<H: Serialize + for<'de> Deserialize<'de>> BlockHeightStore<H> for LmdbBlockHeightStore {
    fn put(&self, height: u64, block_hash: H) -> Result<bool> {
        let serialized_value =
            bincode::serialize(&block_hash).map_err(|error| Error::from_serialization(*error))?;
        let mut txn = self.env.begin_rw_txn().expect("should create rw txn");
        let result = match txn.put(
            self.db,
            &height.to_ne_bytes(),
            &serialized_value,
            WriteFlags::NO_OVERWRITE,
        ) {
            Ok(()) => true,
            Err(lmdb::Error::KeyExist) => false,
            Err(error) => panic!("should put height: {:?}", error),
        };
        let _ = self.highest.fetch_max(height, Ordering::SeqCst);
        txn.commit().expect("should commit txn");
        Ok(result)
    }

    fn get(&self, height: u64) -> Result<Option<H>> {
        let txn = self.env.begin_ro_txn().expect("should create ro txn");
        let serialized_value = match txn.get(self.db, &height.to_ne_bytes()) {
            Ok(value) => value,
            Err(lmdb::Error::NotFound) => return Ok(None),
            Err(error) => panic!("should get: {:?}", error),
        };
        let block_hash = bincode::deserialize(serialized_value)
            .map_err(|error| Error::from_deserialization(*error))?;
        txn.commit().expect("should commit txn");
        Ok(Some(block_hash))
    }

    fn highest(&self) -> Result<Option<H>> {
        let highest = self.highest.load(Ordering::Relaxed);
        self.get(highest)
    }
}
