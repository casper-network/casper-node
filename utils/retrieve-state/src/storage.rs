use std::{convert::TryInto, fs, path::Path, sync::Arc};

use lmdb::{Cursor, Database, DatabaseFlags, Transaction, WriteFlags};

use casper_execution_engine::{
    core::engine_state::{EngineConfig, EngineState},
    storage::{
        global_state::lmdb::LmdbGlobalState,
        transaction_source::{lmdb::LmdbEnvironment, TransactionSource},
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_hashing::Digest;
use casper_node::{
    rpcs::chain::BlockIdentifier,
    types::{Block, BlockHash, Deploy, DeployHash, DeployOrTransferHash, JsonBlock},
};
use casper_types::bytesrepr::{FromBytes, ToBytes};

use crate::{BlockWithDeploys, DEFAULT_MAX_READERS};

pub struct LocalStorage {
    mdb: Arc<LmdbEnvironment>,
    block_hashes_by_height: Database,
    blocks: Database,
    deploys: Database,
}

impl LocalStorage {
    /// Constructor for [`LocalStorage`].
    pub fn create(mdb: Arc<LmdbEnvironment>) -> Result<Self, lmdb::Error> {
        let blocks = mdb
            .env()
            .create_db(Some("blocks"), DatabaseFlags::empty())?;
        let block_hashes_by_height = mdb
            .env()
            .create_db(Some("block_hashes_by_height"), DatabaseFlags::empty())?;
        let deploys = mdb
            .env()
            .create_db(Some("deploys"), DatabaseFlags::empty())?;
        Ok(Self {
            mdb,
            blocks,
            block_hashes_by_height,
            deploys,
        })
    }

    /// Gets the highest height represented in the downloaded blocks.
    pub fn get_highest_height(&self) -> Result<u64, anyhow::Error> {
        let txn = self.mdb.create_read_txn()?;
        let mut cursor = txn.open_ro_cursor(self.block_hashes_by_height)?;
        let mut highest = 0;
        for (key, _) in cursor.iter() {
            let (height, _) = u64::from_bytes(key)?;
            highest = highest.max(height);
        }
        Ok(highest)
    }

    /// Get a block by it's [`BlockIdentifier`].
    pub fn get_block_by_identifier(
        &self,
        identifier: &BlockIdentifier,
    ) -> Result<Option<Block>, anyhow::Error> {
        match identifier {
            BlockIdentifier::Hash(ref block_hash) => self.get_block_by_hash(block_hash),
            BlockIdentifier::Height(ref height) => self.get_block_by_height(*height),
        }
    }

    /// Get a block by height.
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, anyhow::Error> {
        let txn = self.mdb.create_read_txn()?;
        let maybe_block_hash = txn.get(self.block_hashes_by_height, &height.to_bytes()?);
        let (block_hash, _rest) = match maybe_block_hash {
            // It's normal to ask for a block that hasn't been added yet.
            Err(lmdb::Error::NotFound) => return Ok(None),
            result => BlockHash::from_bytes(result?)?,
        };
        let entry_bytes = txn.get(self.blocks, &block_hash.to_bytes()?)?;
        let (block, _rest) = Block::from_bytes(entry_bytes)?;
        Ok(Some(block))
    }

    /// Gets a block from storage by hash.
    pub fn get_block_by_hash(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<Block>, anyhow::Error> {
        let txn = self.mdb.create_read_txn()?;
        let (block, _rest) = match txn.get(self.blocks, block_hash) {
            // It's normal to ask for a block that hasn't been added yet.
            Err(lmdb::Error::NotFound) => return Ok(None),
            result => Block::from_bytes(result?)?,
        };
        Ok(Some(block))
    }

    /// Gets a deploy from storage by hash.
    pub fn get_deploy_by_hash(
        &self,
        deploy_hash: &DeployHash,
    ) -> Result<Option<Deploy>, anyhow::Error> {
        let txn = self.mdb.create_read_txn()?;
        let bytes = txn.get(self.deploys, deploy_hash)?;
        let (deploy, _rest) = Deploy::from_bytes(bytes)?;
        Ok(Some(deploy))
    }

    /// Get a block with it's deploys by [`block-hash`].
    pub fn get_block_with_deploys_by_hash(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockWithDeploys>, anyhow::Error> {
        let block = match self.get_block_by_hash(block_hash)? {
            Some(block) => block,
            None => return Ok(None),
        };
        let deploys = self.get_many_deploys_by_hash(block.deploy_hashes())?;
        let transfers = self.get_many_deploys_by_hash(block.transfer_hashes())?;
        Ok(Some(BlockWithDeploys {
            block: JsonBlock::new(block, None),
            deploys,
            transfers,
        }))
    }

    /// Gets many deploys by hash.
    pub fn get_many_deploys_by_hash(
        &self,
        hashes: &[DeployHash],
    ) -> Result<Vec<Deploy>, anyhow::Error> {
        let mut deploys = vec![];
        for deploy_hash in hashes {
            let deploy = match self.get_deploy_by_hash(deploy_hash)? {
                None => {
                    return Err(anyhow::anyhow!(
                        "Deploy is present in block but hasn't been downloaded."
                    ))
                }
                Some(deploy) => deploy,
            };
            deploys.push(deploy);
        }
        Ok(deploys)
    }

    /// Store a single [`BlockWithDeploys`]'s [`Block`], `deploys` and `transfers`.
    pub fn put_block_with_deploys(
        &self,
        block_with_deploys: &BlockWithDeploys,
    ) -> Result<(), anyhow::Error> {
        let mut txn = self.mdb.create_read_write_txn()?;
        for deploy in block_with_deploys.deploys.iter() {
            if let DeployOrTransferHash::Deploy(hash) = deploy.deploy_or_transfer_hash() {
                txn.put(
                    self.deploys,
                    &hash.to_bytes()?,
                    &deploy.to_bytes()?,
                    WriteFlags::empty(),
                )?;
            } else {
                txn.abort();
                return Err(anyhow::anyhow!("transfer found in list of deploys"));
            }
        }
        for transfer in block_with_deploys.transfers.iter() {
            if let DeployOrTransferHash::Transfer(hash) = transfer.deploy_or_transfer_hash() {
                txn.put(
                    self.deploys,
                    &hash.to_bytes()?,
                    &transfer.to_bytes()?,
                    WriteFlags::empty(),
                )?;
            } else {
                txn.abort();
                return Err(anyhow::anyhow!("deploy found in list of transfers"));
            }
        }
        let block: Block = block_with_deploys.block.clone().try_into()?;
        txn.put(
            self.block_hashes_by_height,
            &block.height().to_bytes()?,
            &block.hash().to_bytes()?,
            WriteFlags::empty(),
        )?;
        txn.put(
            self.blocks,
            &block.hash().to_bytes()?,
            &block.to_bytes()?,
            WriteFlags::empty(),
        )?;

        txn.commit()?;
        Ok(())
    }
}

/// Create an lmdb environment at a given path.
pub fn create_lmdb_environment(
    lmdb_path: impl AsRef<Path>,
    default_max_db_size: usize,
) -> Result<Arc<LmdbEnvironment>, anyhow::Error> {
    let lmdb_environment = Arc::new(LmdbEnvironment::new(
        &lmdb_path,
        default_max_db_size,
        DEFAULT_MAX_READERS,
        true,
        10,
    )?);
    Ok(lmdb_environment)
}

/// Loads an existing execution engine.
pub fn load_execution_engine(
    lmdb_path: impl AsRef<Path>,
    default_max_db_size: usize,
    state_root_hash: Digest,
) -> Result<(Arc<EngineState<LmdbGlobalState>>, Arc<LmdbEnvironment>), anyhow::Error> {
    let lmdb_data_file = lmdb_path.as_ref().join("data.lmdb");
    if !lmdb_path.as_ref().join("data.lmdb").exists() {
        return Err(anyhow::anyhow!(
            "lmdb data file not found at: {}",
            lmdb_data_file.display()
        ));
    }
    let lmdb_environment = create_lmdb_environment(&lmdb_path, default_max_db_size)?;
    let lmdb_trie_store = Arc::new(LmdbTrieStore::open(&lmdb_environment, None)?);
    let global_state = LmdbGlobalState::new(
        Arc::clone(&lmdb_environment),
        lmdb_trie_store,
        state_root_hash,
    );
    Ok((
        Arc::new(EngineState::new(global_state, EngineConfig::default())),
        lmdb_environment,
    ))
}

/// Creates a new execution engine.
pub fn create_execution_engine(
    lmdb_path: impl AsRef<Path>,
    default_max_db_size: usize,
) -> Result<(Arc<EngineState<LmdbGlobalState>>, Arc<LmdbEnvironment>), anyhow::Error> {
    if !lmdb_path.as_ref().exists() {
        println!(
            "creating new lmdb data dir {}",
            lmdb_path.as_ref().display()
        );
        fs::create_dir_all(&lmdb_path)?;
    }
    fs::create_dir_all(&lmdb_path)?;
    let lmdb_environment = create_lmdb_environment(&lmdb_path, default_max_db_size)?;
    lmdb_environment.env().sync(true)?;
    let lmdb_trie_store = Arc::new(LmdbTrieStore::new(
        &lmdb_environment,
        None,
        DatabaseFlags::empty(),
    )?);
    let global_state = LmdbGlobalState::empty(Arc::clone(&lmdb_environment), lmdb_trie_store)?;

    Ok((
        Arc::new(EngineState::new(global_state, EngineConfig::default())),
        lmdb_environment,
    ))
}

#[cfg(test)]
mod tests {
    use crate::{
        storage::{create_lmdb_environment, LocalStorage},
        BlockWithDeploys, DEFAULT_MAX_DB_SIZE,
    };
    use casper_node::rpcs::{
        chain::{BlockIdentifier, GetBlockResult},
        docs::DocExample,
        info::GetDeployResult,
    };
    use testdir::testdir;

    #[test]
    fn block_with_deploys_round_trip_lmdb() {
        let dir = testdir!();
        let lmdb_environment = create_lmdb_environment(&dir, DEFAULT_MAX_DB_SIZE).unwrap();
        let local_storage = LocalStorage::create(lmdb_environment).expect("should create lmdb env");

        let example_deploy = GetDeployResult::doc_example().deploy.clone();
        let example_block = GetBlockResult::doc_example()
            .block
            .as_ref()
            .unwrap()
            .clone();

        let block_with_deploys = BlockWithDeploys {
            block: example_block.clone(),
            transfers: vec![example_deploy.clone()],
            deploys: vec![],
        };

        local_storage
            .put_block_with_deploys(&block_with_deploys)
            .unwrap();
        let stored_block =
            local_storage.get_block_by_identifier(&BlockIdentifier::Hash(example_block.hash));
        assert!(matches!(stored_block, Ok(Some(ref _block))));

        let stored_block_by_height = local_storage
            .get_block_by_identifier(&BlockIdentifier::Height(example_block.header.height));
        assert!(matches!(stored_block, Ok(Some(ref _block))));

        assert_eq!(
            stored_block.unwrap().unwrap(),
            stored_block_by_height.unwrap().unwrap()
        );

        let stored_deploy = local_storage.get_deploy_by_hash(example_deploy.id());
        assert!(matches!(stored_deploy, Ok(Some(_deploy))));
    }
}
