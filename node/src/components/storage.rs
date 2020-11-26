//! Central storage component.
//!
//! The central storage component is in charge of persisting data to disk. Its core functionalities
//! are
//!
//! * storing and loading blocks,
//! * storing and loading deploys,
//! * [temporary until refactored] holding `DeployMetadata` for each deploy,
//! * holding a read-only copy of the chainspec,
//! * keeping an index of blocks by height and
//! * [unimplemented] managing disk usage by pruning blocks and deploys from storage.
//!
//! Any I/O performed by the component is done on the event handling thread, this is on purpose as
//! the assumption is that caching by LMDB will offset any gains from offloading it onto a separate
//! thread, while keeping the maximum event processing time reasonable.
//!
//! ## Consistency
//!
//! The storage upholds a few invariants internally, namely:
//!
//! * [temporary until refactored] Storing an execution result for a deploy in the context of a
//!   block is guaranteed to be idempotent: Storing the same result twice is a no-op, whilst
//!   attempting to store a differing one will cause a fatal error.
//! * Only one block can ever be associated with a specific block height. Attempting to store a
//!   block with a different block already existing at the same height causes a fatal error.
//! * Storing a deploy or block that already exists (same hash) is fine and will silently be
//!   accepted.
//!
//! ## Indices
//!
//! The current implementation keeps only in-memory indices, which are not persisted, based upon the
//! estimate that they are reasonably quick to rebuild on start-up and do not take up much memory.
//!
//! ## Errors
//!
//! The storage component itself is panic free and in general reports three classes of errors:
//! Corruption, temporary resource exhaustion and potential bugs.

mod lmdb_ext;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    fs, io,
    path::PathBuf,
    sync::Arc,
};
#[cfg(test)]
use std::{collections::BTreeSet, convert::TryFrom};

use datasize::DataSize;
use derive_more::From;
use lmdb::{Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use tempfile::TempDir;
use thiserror::Error;
use tracing::info;

use super::Component;
#[cfg(test)]
use crate::crypto::hash::Digest;
use crate::{
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    fatal,
    types::{Block, BlockHash, Deploy, DeployHash, DeployMetadata},
    utils::WithDir,
    Chainspec, NodeRng,
};
use lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt};

/// We can set this very low, as there is only a single reader/writer accessing the component at any
/// one time.
const MAX_TRANSACTIONS: u32 = 1;

/// One Gibibyte.
const GIB: usize = 1024 * 1024 * 1024;

/// Default max block store size.
const DEFAULT_MAX_BLOCK_STORE_SIZE: usize = 450 * GIB;
/// Default max deploy store size.
const DEFAULT_MAX_DEPLOY_STORE_SIZE: usize = 300 * GIB;
/// Default max deploy metadata store size.
const DEFAULT_MAX_DEPLOY_METADATA_STORE_SIZE: usize = 300 * GIB;

/// OS-specific lmdb flags.
#[cfg(not(target_os = "macos"))]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::WRITE_MAP;

/// OS-specific lmdb flags.
///
/// Mac OS X exhibits performance regressions when `WRITE_MAP` is used.
#[cfg(target_os = "macos")]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::empty();

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
    /// Found a duplicate block-at-height index entry.
    #[error("duplicate entries for block at height {height}: {first} / {second}")]
    DuplicateBlockIndex {
        /// Height at which duplication was found.
        height: u64,
        /// First block hash encountered at `height`.
        first: BlockHash,
        /// Second block hash encountered at `height`.
        second: BlockHash,
    },
    /// Attempted to store a duplicate execution result.
    #[error("duplicate execution result for deploy {deploy_hash} in block {block_hash}")]
    DuplicateExecutionResult {
        /// The deploy for which the result should be stored.
        deploy_hash: DeployHash,
        /// The block providing the context for the deploy's execution result.
        block_hash: BlockHash,
    },
    /// LMDB error while operating.
    #[error("internal database error: {0}")]
    InternalStorage(#[from] LmdbExtError),
}

// We wholesale wrap lmdb errors and treat them as internal errors here.
impl From<lmdb::Error> for Error {
    fn from(err: lmdb::Error) -> Self {
        LmdbExtError::from(err).into()
    }
}

#[derive(DataSize, Debug)]
pub struct Storage {
    /// Storage location.
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
    block_height_index: BTreeMap<u64, BlockHash>,
    /// Chainspec cache.
    chainspec_cache: Option<Arc<Chainspec>>,
}

impl<REv> Component<REv> for Storage {
    type Event = Event;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let result = match event {
            Event::StorageRequest(req) => self.handle_storage_request::<REv>(effect_builder, req),
        };

        // Any error is turned into a fatal effect, the component itself does not panic. Note that
        // we are dropping a lot of responders this way, but since we are crashing with fatal
        // anyway, it should not matter.
        match result {
            Ok(effects) => effects,
            Err(err) => fatal!(effect_builder, "storage error: {}", err),
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
                OS_FLAGS |
                // We manage our own directory.
                EnvironmentFlags::NO_SUB_DIR
                // Disable thread local storage, strongly suggested for operation with tokio.
                    | EnvironmentFlags::NO_TLS,
            )
            .set_max_readers(MAX_TRANSACTIONS)
            .set_max_dbs(3)
            .set_map_size(total_size)
            .open(&root.join("storage.lmdb"))?;

        let block_db = env.create_db(Some("blocks"), DatabaseFlags::empty())?;
        let deploy_db = env.create_db(Some("deploys"), DatabaseFlags::empty())?;
        let deploy_metadata_db = env.create_db(Some("deploy_metadata"), DatabaseFlags::empty())?;

        // We now need to restore the block-height index. Log messages allow timing here.
        info!("reindexing block store");
        let mut block_height_index = BTreeMap::new();
        let block_txn = env.begin_ro_txn()?;
        let mut cursor = block_txn.open_ro_cursor(block_db)?;

        // Note: `iter_start` has an undocumented panic if called on an empty database. We rely on
        //       the iterator being at the start when created.
        for (raw_key, raw_val) in cursor.iter() {
            let block: Block = lmdb_ext::deserialize(raw_val)?;
            // We use the opportunity for a small integrity check.
            assert_eq!(
                raw_key,
                block.hash().as_ref(),
                "found corrupt block in database"
            );
            let header = block.header();
            if let Some(duplicate) = block_height_index.insert(header.height(), *block.hash()) {
                // A duplicated block in our backing store causes us to exit early.
                return Err(Error::DuplicateBlockIndex {
                    height: header.height(),
                    first: header.hash(),
                    second: duplicate,
                });
            }
        }
        info!("block store reindexing complete");
        drop(cursor);
        drop(block_txn);

        Ok(Storage {
            root,
            env,
            block_db,
            deploy_db,
            deploy_metadata_db,
            block_height_index,
            chainspec_cache: None,
        })
    }

    /// Handles a storage request.
    fn handle_storage_request<REv>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        req: StorageRequest,
    ) -> Result<Effects<Event>, Error>
    where
        Self: Component<REv>,
    {
        // Note: Database IO is handled in a blocking fashion on purpose throughout this function.
        // The rationale is that long IO operations are very rare and cache misses frequent, so on
        // average the actual execution time will be very low.
        Ok(match req {
            StorageRequest::PutBlock { block, responder } => {
                let mut txn = self.env.begin_rw_txn()?;
                let outcome = txn.put_value(self.block_db, block.hash(), &block, true)?;
                txn.commit()?;

                if outcome {
                    // If we are attempting to insert a duplicate block, return with error.
                    if let Some(first) = self.block_height_index.get(&block.height()) {
                        if first != block.hash() {
                            return Err(Error::DuplicateBlockIndex {
                                height: block.height(),
                                first: *first,
                                second: *block.hash(),
                            });
                        }
                    }
                }

                self.block_height_index
                    .insert(block.height(), *block.hash());

                responder.respond(outcome).ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => responder
                .respond(self.get_single_block(&mut self.env.begin_ro_txn()?, &block_hash)?)
                .ignore(),
            StorageRequest::GetBlockAtHeight { height, responder } => responder
                .respond(self.get_block_by_height(&mut self.env.begin_ro_txn()?, height)?)
                .ignore(),
            StorageRequest::GetHighestBlock { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(
                        self.block_height_index
                            .keys()
                            .last()
                            .and_then(|&height| {
                                self.get_block_by_height(&mut txn, height).transpose()
                            })
                            .transpose()?,
                    )
                    .ignore()
            }
            StorageRequest::GetBlockHeader {
                block_hash,
                responder,
            } => responder
                // TODO: Find a solution for efficiently retrieving the blocker header without the
                // block. Deserialization that allows trailing bytes could be a possible solution.
                .respond(
                    self.get_single_block(&mut self.env.begin_ro_txn()?, &block_hash)?
                        .map(|block| block.header().clone()),
                )
                .ignore(),
            StorageRequest::PutDeploy { deploy, responder } => {
                let mut txn = self.env.begin_rw_txn()?;
                let outcome = txn.put_value(self.deploy_db, deploy.id(), &deploy, false)?;
                txn.commit()?;
                responder.respond(outcome).ignore()
            }
            StorageRequest::GetDeploys {
                deploy_hashes,
                responder,
            } => responder
                .respond(self.get_deploys(&mut self.env.begin_ro_txn()?, deploy_hashes.as_slice())?)
                .ignore(),
            StorageRequest::GetDeployHeaders {
                deploy_hashes,
                responder,
            } => responder
                .respond(
                    // TODO: Similarly to getting block headers, requires optimized function.
                    self.get_deploys(&mut self.env.begin_ro_txn()?, deploy_hashes.as_slice())?
                        .into_iter()
                        .map(|opt| opt.map(|deploy| deploy.header().clone()))
                        .collect(),
                )
                .ignore(),
            StorageRequest::PutExecutionResults {
                block_hash,
                execution_results,
                responder,
            } => {
                let mut txn = self.env.begin_rw_txn()?;

                for (deploy_hash, execution_result) in execution_results {
                    let mut metadata = self
                        .get_deploy_metadata(&mut txn, &deploy_hash)?
                        .unwrap_or_default();

                    // If we have a previous execution result, we enforce that it is the same.
                    if let Some(prev) = metadata.execution_results.get(&block_hash) {
                        if prev != &execution_result {
                            return Err(Error::DuplicateExecutionResult {
                                deploy_hash,
                                block_hash,
                            });
                        }

                        // We can now skip adding, as the result is the same.
                        continue;
                    }

                    // Update metadata and write back to db.
                    metadata
                        .execution_results
                        .insert(block_hash, execution_result);
                    let was_written =
                        txn.put_value(self.deploy_metadata_db, &deploy_hash, &metadata, true)?;
                    assert!(was_written);
                }

                txn.commit()?;
                responder.respond(()).ignore()
            }
            StorageRequest::GetDeployAndMetadata {
                deploy_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;

                // A missing deploy causes an early `None` return.
                let deploy: Deploy =
                    if let Some(deploy) = txn.get_value(self.deploy_db, &deploy_hash)? {
                        deploy
                    } else {
                        return Ok(responder.respond(None).ignore());
                    };

                // Missing metadata is filled using a default.
                let metadata = self
                    .get_deploy_metadata(&mut txn, &deploy_hash)?
                    .unwrap_or_default();
                responder.respond(Some((deploy, metadata))).ignore()
            }
            StorageRequest::PutChainspec {
                chainspec,
                responder,
            } => {
                self.chainspec_cache = Some(chainspec);

                responder.respond(()).ignore()
            }
            StorageRequest::GetChainspec {
                version: _version,
                responder,
            } => responder.respond(self.chainspec_cache.clone()).ignore(),
        })
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    fn get_block_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        height: u64,
    ) -> Result<Option<Block>, LmdbExtError> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves a single block in a separate transaction from storage.
    fn get_single_block<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Block>, LmdbExtError> {
        tx.get_value(self.block_db, &block_hash)
    }

    /// Retrieves a set of deploys from storage.
    fn get_deploys<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        deploy_hashes: &[DeployHash],
    ) -> Result<Vec<Option<Deploy>>, LmdbExtError> {
        deploy_hashes
            .iter()
            .map(|deploy_hash| tx.get_value(self.deploy_db, deploy_hash))
            .collect()
    }

    /// Retrieves deploy metadata associated with deploy.
    ///
    /// If no deploy metadata is stored for the specific deploy, an empty metadata instance will be
    /// created, but not stored.
    fn get_deploy_metadata<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        deploy_hash: &DeployHash,
    ) -> Result<Option<DeployMetadata>, Error> {
        Ok(tx.get_value(self.deploy_metadata_db, deploy_hash)?)
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
    /// The maximum size of the database to use for the deploy metadata store.
    ///
    /// The size should be a multiple of the OS page size.
    max_deploy_metadata_store_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // No one should be instantiating a config with storage set to default.
            path: "/dev/null".into(),
            max_block_store_size: DEFAULT_MAX_BLOCK_STORE_SIZE,
            max_deploy_store_size: DEFAULT_MAX_DEPLOY_STORE_SIZE,
            max_deploy_metadata_store_size: DEFAULT_MAX_DEPLOY_METADATA_STORE_SIZE,
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

// Legacy code follows.
//
// The functionality about for requests directly from the incoming network was previously present in
// the validator reactor's routing code. It is slated for an overhaul, but for the time being the
// code below provides a backwards-compatible interface for this functionality. DO NOT EXPAND, RELY
// ON OR BUILD UPON THIS CODE.

impl Storage {
    // Retrieves a deploy from the deploy store to handle a legacy network request.
    pub fn handle_legacy_direct_deploy_request(&self, deploy_hash: DeployHash) -> Option<Deploy> {
        // NOTE: This function was formerly called `get_deploy_for_peer` and used to create an event
        // directly. This caused a dependency of the storage component on networking functionality,
        // which is highly problematic. For this reason, the code to send a reply has been moved to
        // the dispatching code (which should be removed anyway) as to not taint the interface.
        self.env
            .begin_ro_txn()
            .map_err(Into::into)
            .and_then(|mut tx| tx.get_value(self.deploy_db, &deploy_hash))
            .expect("legacy direct deploy request failed")
    }
}

// Testing code. The functions below allow direct inspection of the storage component and should
// only ever be used when writing tests.
#[cfg(test)]
impl Storage {
    /// Directly returns a deploy from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub fn get_deploy_by_hash(&self, deploy_hash: DeployHash) -> Option<Deploy> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        txn.get_value(self.deploy_db, &deploy_hash)
            .expect("could not retrieve value from storage")
    }

    /// Reads all known deploy hashes from the internal store.
    ///
    /// # Panics
    ///
    /// Panics on any IO or db corruption error.
    pub fn get_all_deploy_hashes(&self) -> BTreeSet<DeployHash> {
        let txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");

        let mut cursor = txn
            .open_ro_cursor(self.deploy_db)
            .expect("could not create cursor");

        cursor
            .iter()
            .map(|(raw_key, _)| {
                DeployHash::new(Digest::try_from(raw_key).expect("malformed deploy hash in DB"))
            })
            .collect()
    }
}
