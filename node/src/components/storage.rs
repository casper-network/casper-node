//! Central storage component.
//!
//! The central storage component is in charge of persisting data to disk. Its core functionalities
//! are
//!
//! * storing and loading blocks,
//! * storing and loading deploys,
//! * holding a read-only copy of the chainspec,
//! * keeping an index of blocks by height and
//! * [unimplemented] managing disk usage by pruning blocks and deploys from storage.

mod lmdb_ext;
mod serialization;

use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    fs, io,
    path::PathBuf,
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;
use lmdb::{Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{block_proposer::BlockProposerState, Component};
use crate::{
    effect::{requests::StorageRequest, EffectExt, Effects},
    types::{Block, BlockHash, Deploy, DeployHash, DeployMetadata, Timestamp},
    utils::WithDir,
    Chainspec,
};
use lmdb_ext::{EnvironmentExt, TransactionExt, WriteTransactionExt};
use serialization::{deser, ser};

#[cfg(test)]
use tempfile::TempDir;
use tracing::{info, warn};

/// We can set this very low, as there is only a single reader/writer accessing the component at any
/// one time.
const MAX_TRANSACTIONS: u32 = 4;

/// Where to store the chainspec in.
const CHAINSPEC_CACHE_FILENAME: &str = "chainspec_cache";

#[derive(Debug, From)]
pub enum Event {
    /// Incoming storage request.
    #[from]
    StorageRequest(StorageRequest),
}

/// A storage component initialization error.
#[derive(Debug, Error)]
pub enum Error {
    /// Failure to create the root database directory.
    #[error("failed to create database directory `{}`: {}", .0.display(), .1)]
    CreateDatabaseDirectory(PathBuf, io::Error),
    #[error("failed to initialize lmdb: {}", .0)]
    /// LMDB initialization failure.
    LmdbInit(lmdb::Error),
}

#[derive(DataSize, Debug)]
pub struct Storage {
    /// Storage location.
    #[data_size(skip)]
    root: PathBuf,
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    env: Environment,
    /// The block database.
    #[data_size(skip)]
    block_db: Database,
    /// The deploy database.
    #[data_size(skip)]
    deploy_db: Database,
    /// The deploy metadata database.
    #[data_size(skip)]
    deploy_metadata_db: Database,
    /// Block height index.
    #[data_size(skip)]
    block_height_index: BTreeMap<u64, BlockHash>,
    /// Chainspec chache.
    #[data_size(skip)]
    chainspec_cache: Option<Arc<Chainspec>>,
}

impl<REv> Component<REv> for Storage {
    type Event = Event;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        _effect_builder: crate::effect::EffectBuilder<REv>,
        _rng: &mut dyn crate::types::CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::StorageRequest(req) => self.handle_storage_request::<REv>(req),
        }
    }
}

impl Storage {
    /// Creates a new storage component.
    pub(crate) fn new(cfg: &WithDir<Config>) -> Result<Self, Error> {
        let config = cfg.value();

        // Create the database directory.
        let root = cfg.with_dir(config.path.clone());

        if !root.exists() {
            fs::create_dir_all(&root)
                .map_err(|err| Error::CreateDatabaseDirectory(root.clone(), err))?;
        }

        // Calculate the upper bound for the memory map that is potentially used.
        let total_size = config
            .max_block_store_size
            .saturating_add(config.max_deploy_store_size)
            .saturating_add(config.max_deploy_metadata_store_size);

        // Creates the environment and databases.
        let env = Environment::new()
            .set_flags(
                EnvironmentFlags::NO_SUB_DIR
                    | EnvironmentFlags::NO_TLS
                    | EnvironmentFlags::WRITE_MAP,
            )
            .set_max_readers(MAX_TRANSACTIONS)
            .set_max_dbs(4)
            .set_map_size(total_size)
            .open(&root.join("storage.lmdb"))
            .map_err(Error::LmdbInit)?;

        let block_db = env
            .create_db(Some("blocks"), DatabaseFlags::empty())
            .map_err(Error::LmdbInit)?;
        let deploy_db = env
            .create_db(Some("deploys"), DatabaseFlags::empty())
            .map_err(Error::LmdbInit)?;
        let deploy_metadata_db = env
            .create_db(Some("deploy_metadata"), DatabaseFlags::empty())
            .map_err(Error::LmdbInit)?;

        // Load chainspec from file. A corrupt chainspec will lead to a panic.
        let chainspec_cache = if let Ok(raw) = fs::read(root.join(CHAINSPEC_CACHE_FILENAME)) {
            Some(deser(&raw))
        } else {
            None
        };

        // We now need to restore the block-height index. Log messages allow timing here.
        info!("reindexing block store");
        let mut block_height_index = BTreeMap::new();
        let block_tx = env.ro_transaction();
        let mut cursor = block_tx
            .open_ro_cursor(block_db)
            .expect("could not create read-only cursor on block store");

        // Note: `iter_start` has an undocument panic if called on an empty database. We rely on
        //       the iterator being at the start when created.
        for (raw_key, raw_val) in cursor.iter() {
            let block: Block = deser(raw_val);
            // We use the opportunity for a small integrity check.
            assert_eq!(
                raw_key,
                block.hash().as_ref(),
                "found corrupt block in database"
            );
            let header = block.header();
            if let Some(duplicate) = block_height_index.insert(header.height(), *block.hash()) {
                warn!(
                    height = %header.height(),
                    hash_a = %header.hash(),
                    hash_b = %duplicate,
                    "found duplicate height in block database",
                );
            }
        }
        info!("block store reindexing complete");
        drop(cursor);
        drop(block_tx);

        Ok(Storage {
            root,
            env,
            block_db,
            deploy_db,
            deploy_metadata_db,
            block_height_index,
            chainspec_cache,
        })
    }

    /// Handles a storage request.
    fn handle_storage_request<REv>(
        &mut self,
        req: StorageRequest,
    ) -> Effects<<Self as Component<REv>>::Event>
    where
        Self: Component<REv>,
    {
        // Note: Database IO is handled in a blocking fashion on purpose throughout this function.
        // The rationale behind is that long IO operations are very rare and cache misses
        // frequent, so on average the actual execution time will be very low.
        match req {
            StorageRequest::PutBlock { block, responder } => {
                let mut tx = self.env.rw_transaction();
                let outcome = tx.put_value(self.block_db, block.hash(), &block);
                tx.commit_ok();

                if outcome {
                    if let Some(prev) = self
                        .block_height_index
                        .insert(block.height(), *block.hash())
                    {
                        warn!(height = %block.height(),
                              new=%block.hash(),
                              %prev,
                              "duplicate block for height inserted")
                    }
                }

                responder.respond(outcome).ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => responder
                .respond(self.get_single_block(&block_hash))
                .ignore(),
            StorageRequest::GetBlockAtHeight { height, responder } => {
                responder.respond(self.get_block_by_height(height)).ignore()
            }
            StorageRequest::GetHighestBlock { responder } => responder
                .respond(
                    self.block_height_index
                        .keys()
                        .last()
                        .and_then(|&height| self.get_block_by_height(height)),
                )
                .ignore(),
            StorageRequest::GetBlockHeader {
                block_hash,
                responder,
            } => responder
                .respond(
                    self.get_single_block(&block_hash)
                        .map(|block| block.header().clone()),
                )
                .ignore(),
            StorageRequest::PutDeploy { deploy, responder } => {
                let mut tx = self.env.rw_transaction();
                let outcome = tx.put_value(self.deploy_db, deploy.id(), &deploy);
                tx.commit_ok();
                responder.respond(outcome).ignore()
            }
            StorageRequest::GetDeploys {
                deploy_hashes,
                responder,
            } => responder
                .respond(self.get_deploys(deploy_hashes.as_slice()))
                .ignore(),
            StorageRequest::GetDeployHeaders {
                deploy_hashes,
                responder,
            } => responder
                .respond(
                    self.get_deploys(deploy_hashes.as_slice())
                        .into_iter() // TODO: Ineffecient, a dedicated function can avoid reallocation.
                        .map(|opt| opt.map(|deploy| deploy.header().clone()))
                        .collect(),
                )
                .ignore(),
            StorageRequest::PutExecutionResults {
                block_hash,
                execution_results,
                responder,
            } => {
                // TODO: Verify this code is working as intended.
                let mut tx = self.env.rw_transaction();

                let mut metadata: DeployMetadata = tx
                    .get_value(self.deploy_metadata_db, &block_hash)
                    .unwrap_or_default();

                // If we already have this execution result, return false.
                let mut total_new = execution_results.len();
                for (_deploy_hash, execution_result) in execution_results {
                    if metadata
                        .execution_results
                        .insert(block_hash, execution_result)
                        .is_some()
                    {
                        total_new -= 1;
                    }
                }

                // Store the updated metadata.
                if total_new > 0 {
                    tx.put_value(self.deploy_metadata_db, &block_hash, &metadata);
                    tx.commit_ok();
                }

                responder.respond(total_new).ignore()
            }
            StorageRequest::GetDeployAndMetadata {
                deploy_hash,
                responder,
            } => {
                let mut tx = self.env.ro_transaction();

                let value = tx.get_value(self.deploy_db, &deploy_hash).map(|deploy| {
                    (
                        deploy,
                        tx.get_value(self.deploy_metadata_db, &deploy_hash)
                            .unwrap_or_default(),
                    )
                });
                responder.respond(value).ignore()
            }

            StorageRequest::PutChainspec {
                chainspec,
                responder,
            } => {
                // Commit to storage first, then update cache.
                let chainspec_file = fs::File::create(self.root.join(CHAINSPEC_CACHE_FILENAME))
                    .expect("could not create chainspec cache file");
                ser(chainspec_file, &*chainspec).expect("could not update chainspec cache");
                self.chainspec_cache = Some(chainspec);

                responder.respond(()).ignore()
            }
            StorageRequest::GetChainspec {
                version: _version,
                responder,
            } => responder.respond(self.chainspec_cache.clone()).ignore(),
        }
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    fn get_block_by_height(&self, height: u64) -> Option<Block> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block(block_hash))
    }

    /// Retrieves a single block in a separate transaction from storage.
    fn get_single_block(&self, block_hash: &BlockHash) -> Option<Block> {
        let mut tx = self.env.ro_transaction();
        tx.get_value(self.block_db, &block_hash)
    }

    /// Retrieves a set of deploys from storage.
    fn get_deploys(&self, deploy_hashes: &[DeployHash]) -> Vec<Option<Deploy>> {
        let mut tx = self.env.ro_transaction();

        deploy_hashes
            .iter()
            .map(|deploy_hash| tx.get_value(self.deploy_db, deploy_hash))
            .collect()
    }

    /// TODO: What is this?
    pub(crate) async fn load_block_proposer_state(
        &self,
        _latest_block_height: u64,
        _chainspec_version: Version,
        _timestamp: Timestamp,
    ) -> BlockProposerState {
        // TODO: Re-evaluate if this functionality can be scrapped.
        todo!()
    }
}

/// On-disk storage configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The path to the folder where any files created or read by the storage component will exist.
    ///
    /// If the folder doesn't exist, it and any required parents will be created.
    pub(crate) path: PathBuf,
    /// The maximum size of the database to use for the block store.
    ///
    /// The size should be a multiple of the OS page size.
    max_block_store_size: usize,
    /// The maximum size of the database to use for the deploy store.
    ///
    /// The size should be a multiple of the OS page size.
    max_deploy_store_size: usize,
    /// The maximum size of the database to use for the deploy store.
    ///
    /// The size should be a multiple of the OS page size.
    max_deploy_metadata_store_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // No one should be instantiating a config with storage set to default.
            path: "/dev/null".into(),
            max_block_store_size: 483_183_820_800,
            max_deploy_store_size: 322_122_547_200,
            max_deploy_metadata_store_size: 322_122_547_200,
        }
    }
}

impl Config {
    /// Returns a default `Config` suitable for tests, along with a `TempDir` which must be kept
    /// alive for the duration of the test since its destructor removes the dir from the filesystem.
    #[cfg(test)]
    pub(crate) fn default_for_tests() -> (Self, TempDir) {
        let tempdir = tempfile::tempdir().expect("should get tempdir");
        let path = tempdir.path().join("lmdb");

        let config = Config {
            path,
            ..Default::default()
        };
        (config, tempdir)
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::StorageRequest(req) => req.fmt(f),
        }
    }
}
