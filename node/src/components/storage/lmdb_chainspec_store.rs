use std::{fmt::Debug, path::Path};

use lmdb::{self, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction, WriteFlags};
use semver::Version;
use tracing::info;

use super::{ChainspecStore, Error, Result};
use crate::Chainspec;

/// LMDB version of a store.
#[derive(Debug)]
pub(super) struct LmdbChainspecStore {
    env: Environment,
    db: Database,
}

impl LmdbChainspecStore {
    pub(crate) fn new<P: AsRef<Path>>(db_path: P, max_size: usize) -> Result<Self> {
        let env = Environment::new()
            .set_flags(EnvironmentFlags::NO_SUB_DIR)
            .set_map_size(max_size)
            .open(db_path.as_ref())?;
        let db = env.create_db(None, DatabaseFlags::empty())?;
        info!("opened DB at {}", db_path.as_ref().display());

        Ok(LmdbChainspecStore { env, db })
    }
}

impl ChainspecStore for LmdbChainspecStore {
    fn put(&self, chainspec: Chainspec) -> Result<()> {
        let id = bincode::serialize(&chainspec.genesis.protocol_version)
            .map_err(|error| Error::from_serialization(*error))?;
        let serialized_value =
            bincode::serialize(&chainspec).map_err(|error| Error::from_serialization(*error))?;
        let mut txn = self.env.begin_rw_txn()?;
        txn.put(self.db, &id, &serialized_value, WriteFlags::empty())?;
        txn.commit()?;
        Ok(())
    }

    fn get(&self, version: Version) -> Result<Chainspec> {
        let id = bincode::serialize(&version).map_err(|error| Error::from_serialization(*error))?;
        let txn = self.env.begin_ro_txn()?;
        let serialized_value = match txn.get(self.db, &id) {
            Ok(value) => value,
            Err(lmdb::Error::NotFound) => return Err(Error::NotFound),
            Err(error) => return Err(error.into()),
        };
        let value = bincode::deserialize(serialized_value)
            .map_err(|error| Error::from_deserialization(*error))?;
        txn.commit()?;
        Ok(value)
    }
}
