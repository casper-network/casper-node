use std::{fmt::Debug, path::Path};

use lmdb::{self, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction, WriteFlags};
use tracing::info;

use super::{
    block_proposer_state_store::{BlockProposerStateStore, BLOCK_PROPOSER_STATE_KEY},
    Error, Result,
};
use crate::{components::block_proposer::BlockProposerState, MAX_THREAD_COUNT};

/// LMDB version of a store.
#[derive(Debug)]
pub(super) struct LmdbBlockProposerStateStore {
    env: Environment,
    db: Database,
}

impl LmdbBlockProposerStateStore {
    pub(crate) fn new<P: AsRef<Path>>(db_path: P, max_size: usize) -> Result<Self> {
        let env = Environment::new()
            .set_flags(EnvironmentFlags::NO_SUB_DIR)
            .set_map_size(max_size)
            // to avoid panic on excessive read-only transactions
            .set_max_readers(MAX_THREAD_COUNT as u32)
            .open(db_path.as_ref())?;
        let db = env.create_db(None, DatabaseFlags::empty())?;
        info!("opened DB at {}", db_path.as_ref().display());

        Ok(LmdbBlockProposerStateStore { env, db })
    }
}

impl BlockProposerStateStore for LmdbBlockProposerStateStore {
    fn put(&self, block_proposer: BlockProposerState) -> Result<()> {
        let id = bincode::serialize(&BLOCK_PROPOSER_STATE_KEY)
            .map_err(|error| Error::from_serialization(*error))?;
        let serialized_value = bincode::serialize(&block_proposer)
            .map_err(|error| Error::from_serialization(*error))?;
        let mut txn = self.env.begin_rw_txn().expect("should create rw txn");
        txn.put(self.db, &id, &serialized_value, WriteFlags::empty())
            .expect("should put");
        txn.commit().expect("should commit txn");
        Ok(())
    }

    fn get(&self) -> Result<Option<BlockProposerState>> {
        let id = bincode::serialize(&BLOCK_PROPOSER_STATE_KEY)
            .map_err(|error| Error::from_serialization(*error))?;
        let txn = self.env.begin_ro_txn().expect("should create ro txn");
        let serialized_value = match txn.get(self.db, &id) {
            Ok(value) => value,
            Err(lmdb::Error::NotFound) => return Ok(None),
            Err(error) => panic!("should get: {:?}", error),
        };
        let value = bincode::deserialize(serialized_value)
            .map_err(|error| Error::from_deserialization(*error))?;
        txn.commit().expect("should commit txn");
        Ok(Some(value))
    }
}
