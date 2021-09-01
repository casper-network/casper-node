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

mod blob_cache;
mod lmdb_ext;

#[cfg(test)]
mod tests;

#[cfg(test)]
use std::collections::BTreeSet;
use std::{
    collections::{btree_map::Entry, BTreeMap, HashSet},
    convert::TryFrom,
    fmt::{self, Display, Formatter},
    fs, io, mem,
    path::{Path, PathBuf},
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;
use lmdb::{
    Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, RwTransaction, Transaction,
    WriteFlags,
};
use serde::{Deserialize, Serialize};
use static_assertions::const_assert;
#[cfg(test)]
use tempfile::TempDir;
use thiserror::Error;
use tracing::{debug, error, info, warn};

use casper_execution_engine::shared::newtypes::Blake2bHash;
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    Digest, EraId, ExecutionResult, ProtocolVersion, PublicKey, Transfer, Transform,
};

use crate::{
    components::Component,
    crypto,
    crypto::hash::{self, Digest},
    effect::{
        requests::{StateStoreRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    reactor::ReactorEvent,
    types::{
        error::BlockValidationError, Block, BlockBody, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockSignatures, Deploy, DeployHash, DeployHeader, DeployMetadata,
        HashingAlgorithmVersion, Item, MerkleBlockBody, MerkleBlockBodyPart, MerkleLinkedListNode,
        SharedObject, TimeDiff,
    },
    utils::{display_error, WithDir},
    NodeRng,
};
use blob_cache::BlobCache;
use lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt};

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";

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
/// Default max state store size.
const DEFAULT_MAX_STATE_STORE_SIZE: usize = 10 * GIB;
/// Maximum number of allowed dbs.
const MAX_DB_COUNT: u32 = 11;

/// OS-specific lmdb flags.
#[cfg(not(target_os = "macos"))]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::WRITE_MAP;

/// OS-specific lmdb flags.
///
/// Mac OS X exhibits performance regressions when `WRITE_MAP` is used.
#[cfg(target_os = "macos")]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::empty();
const _STORAGE_EVENT_SIZE: usize = mem::size_of::<Event>();
const_assert!(_STORAGE_EVENT_SIZE <= 96);

const STORAGE_FILES: [&str; 5] = [
    "data.lmdb",
    "data.lmdb-lock",
    "storage.lmdb",
    "storage.lmdb-lock",
    "sse_index",
];

#[derive(Debug, From, Serialize)]
#[repr(u8)]
pub(crate) enum Event {
    /// Incoming storage request.
    #[from]
    StorageRequest(StorageRequest),
    /// Incoming state storage request.
    #[from]
    StateStoreRequest(StateStoreRequest),
}

/// A storage component error.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Failure to create the root database directory.
    #[error("failed to create database directory `{}`: {}", .0.display(), .1)]
    CreateDatabaseDirectory(PathBuf, io::Error),
    /// Found a duplicate block-at-height index entry.
    #[error("duplicate entries for block at height {height}: {first} / {second}")]
    DuplicateBlockIndex {
        /// Height at which duplicate was found.
        height: u64,
        /// First block hash encountered at `height`.
        first: BlockHash,
        /// Second block hash encountered at `height`.
        second: BlockHash,
    },
    /// Found a duplicate switch-block-at-era-id index entry.
    #[error("duplicate entries for switch block at era id {era_id}: {first} / {second}")]
    DuplicateEraIdIndex {
        /// Era ID at which duplicate was found.
        era_id: EraId,
        /// First block hash encountered at `era_id`.
        first: BlockHash,
        /// Second block hash encountered at `era_id`.
        second: BlockHash,
    },
    /// Found a duplicate switch-block-at-era-id index entry.
    #[error("duplicate entries for blocks for deploy {deploy_hash}: {first} / {second}")]
    DuplicateDeployIndex {
        /// Deploy hash at which duplicate was found.
        deploy_hash: DeployHash,
        /// First block hash encountered at `deploy_hash`.
        first: BlockHash,
        /// Second block hash encountered at `deploy_hash`.
        second: BlockHash,
    },
    /// LMDB error while operating.
    #[error("internal database error: {0}")]
    InternalStorage(#[from] LmdbExtError),
    /// Filesystem error while trying to move file.
    #[error("unable to move file {source_path} to {dest_path}: {original_error}")]
    UnableToMoveFile {
        /// The path to the file that should have been moved.
        source_path: PathBuf,
        /// The path where the file should have been moved to.
        dest_path: PathBuf,
        /// The original `io::Error` from `fs::rename`.
        original_error: io::Error,
    },
    /// Mix of missing and found storage files.
    #[error("expected files to exist: {missing_files:?}.")]
    MissingStorageFiles {
        /// The files that were not be found in the storage directory.
        missing_files: Vec<PathBuf>,
    },
    /// Error when validating a block.
    #[error(transparent)]
    BlockValidation(#[from] BlockValidationError),
    /// A block header was not stored under its hash.
    #[error(
        "Block header not stored under its hash. \
         Queried block hash bytes: {queried_block_hash_bytes:x?}, \
         Found block header hash bytes: {found_block_header_hash:x?}, \
         Block header: {block_header}"
    )]
    BlockHeaderNotStoredUnderItsHash {
        /// The queried block hash.
        queried_block_hash_bytes: Vec<u8>,
        /// The actual header of the block hash.
        found_block_header_hash: BlockHash,
        /// The block header found in storage.
        block_header: Box<BlockHeader>,
    },
    /// Block body did not have a block header.
    #[error(
        "No block header corresponding to block body found in LMDB. \
         Block body hash: {block_body_hash:?}, \
         Hashing algorithm version: {hashing_algorithm_version:?}, \
         Block body: {block_body:?}"
    )]
    NoBlockHeaderForBlockBody {
        /// The block body hash.
        block_body_hash: Digest,
        /// The hashing algorithm of the block body.
        hashing_algorithm_version: HashingAlgorithmVersion,
        /// The block body.
        block_body: Box<BlockBody>,
    },
    /// Unexpected hashing algorithm version.
    #[error(
        "Unexpected hashing algorithm version. \
         Expected: {expected_hashing_algorithm_version:?}, \
         Actual: {actual_hashing_algorithm_version:?}"
    )]
    UnexpectedHashingAlgorithmVersion {
        /// Expected hashing algorithm version.
        expected_hashing_algorithm_version: HashingAlgorithmVersion,
        /// Actual hashing algorithm version.
        actual_hashing_algorithm_version: HashingAlgorithmVersion,
    },
    /// Could not find block body part.
    #[error(
        "Could not find block body part with Merkle linked list node hash: \
         {merkle_linked_list_node_hash:?}"
    )]
    CouldNotFindBlockBodyPart {
        /// The block hash queried.
        block_hash: BlockHash,
        /// The hash of the node in the Merkle linked list.
        merkle_linked_list_node_hash: Digest,
    },
    /// Could not verify finality signatures for block.
    #[error("{0} in signature verification. Database is corrupted.")]
    SignatureVerification(crypto::Error),
    /// Corrupted block signature index.
    #[error(
        "Block signatures not indexed by their block hash. \
         Key bytes in LMDB: {raw_key:x?}, \
         Block hash bytes in record: {block_hash_bytes:x?}"
    )]
    CorruptedBlockSignatureIndex {
        /// The key in the block signature index.
        raw_key: Vec<u8>,
        /// The block hash of the signatures found in the index.
        block_hash_bytes: Vec<u8>,
    },
    /// Non-existent block body referred to by header.
    #[error("Non-existent block body referred to by header. Block header: {0:?}")]
    NonExistentBlockBodyReferredToByHeader(Box<BlockHeader>),
    /// Non-existent deploy or transfer in block.
    #[error(
        "Non-existent deploy or transfer in block. \
         Deploy hash: {deploy_hash:?}, \
         Block: {block:?}"
    )]
    NonExistentDeploy {
        /// The missing deploy hash.
        deploy_hash: DeployHash,
        /// The block which has the missing deploy hash.
        block: Box<Block>,
    },
    /// A block body was found to have more parts than expected.
    #[error(
        "Found an unexpected part of a block body in the database: \
        {part_hash:?}"
    )]
    UnexpectedBlockBodyPart {
        /// The block body with the issue.
        block_body_hash: Digest,
        /// The hash of the superfluous body part.
        part_hash: Digest,
    },
}

// We wholesale wrap lmdb errors and treat them as internal errors here.
impl From<lmdb::Error> for Error {
    fn from(err: lmdb::Error) -> Self {
        LmdbExtError::from(err).into()
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct Storage {
    /// Storage location.
    root: PathBuf,
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    env: Environment,
    /// The block header database.
    #[data_size(skip)]
    block_header_db: Database,
    /// The block body database.
    #[data_size(skip)]
    block_body_v1_db: Database,
    /// The Merkle-hashed block body database.
    #[data_size(skip)]
    block_body_v2_db: Database,
    /// The deploy hashes database.
    #[data_size(skip)]
    deploy_hashes_db: Database,
    /// The transfer hashes database.
    #[data_size(skip)]
    transfer_hashes_db: Database,
    /// The proposers database.
    #[data_size(skip)]
    proposer_db: Database,
    /// The block metadata db.
    #[data_size(skip)]
    block_metadata_db: Database,
    /// The deploy database.
    #[data_size(skip)]
    deploy_db: Database,
    /// The deploy metadata database.
    #[data_size(skip)]
    deploy_metadata_db: Database,
    /// The transfer database.
    #[data_size(skip)]
    transfer_db: Database,
    /// The state storage database.
    #[data_size(skip)]
    state_store_db: Database,
    /// A map of block height to block ID.
    block_height_index: BTreeMap<u64, BlockHash>,
    /// A map of era ID to switch block ID.
    switch_block_era_id_index: BTreeMap<EraId, BlockHash>,
    /// A map of deploy hashes to hashes of blocks containing them.
    deploy_hash_index: BTreeMap<DeployHash, BlockHash>,
    /// Whether or not memory deduplication is enabled.
    enable_mem_deduplication: bool,
    /// Pool of loaded items.
    deploy_cache: BlobCache<<Deploy as Item>::Id>,
}

impl<REv> Component<REv> for Storage
where
    REv: ReactorEvent,
{
    type Event = Event;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let result = match event {
            Event::StorageRequest(req) => self.handle_storage_request::<REv>(req),
            Event::StateStoreRequest(req) => {
                self.handle_state_store_request::<REv>(effect_builder, req)
            }
        };

        // Any error is turned into a fatal effect, the component itself does not panic. Note that
        // we are dropping a lot of responders this way, but since we are crashing with fatal
        // anyway, it should not matter.
        match result {
            Ok(effects) => effects,
            Err(err) => fatal!(effect_builder, "storage error: {}", err).ignore(),
        }
    }
}

impl Storage {
    /// Creates a new storage component.
    ///
    /// If `should_check_integrity` is true, time-consuming integrity checks will be performed
    /// during this call to `new()`, potentially blocking for several minutes.  This should normally
    /// only be required if the node is detected to have restarted after a crash.
    pub(crate) fn new(
        cfg: &WithDir<Config>,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
        should_check_integrity: bool,
        network_name: &str,
    ) -> Result<Self, Error> {
        let config = cfg.value();

        // Create the database directory.
        let mut root = cfg.with_dir(config.path.clone());
        let network_subdir = root.join(network_name);

        if !network_subdir.exists() {
            fs::create_dir_all(&network_subdir)
                .map_err(|err| Error::CreateDatabaseDirectory(network_subdir.clone(), err))?;
        }

        if should_move_storage_files_to_network_subdir(&root, &STORAGE_FILES)? {
            move_storage_files_to_network_subdir(&root, &network_subdir, &STORAGE_FILES)?;
        }

        root = network_subdir;

        // Calculate the upper bound for the memory map that is potentially used.
        let total_size = config
            .max_block_store_size
            .saturating_add(config.max_deploy_store_size)
            .saturating_add(config.max_deploy_metadata_store_size);

        // Creates the environment and databases.
        let env = Environment::new()
            .set_flags(
                OS_FLAGS
                // We manage our own directory.
                | EnvironmentFlags::NO_SUB_DIR
                // Disable thread local storage, strongly suggested for operation with tokio.
                | EnvironmentFlags::NO_TLS,
            )
            .set_max_readers(MAX_TRANSACTIONS)
            .set_max_dbs(MAX_DB_COUNT)
            .set_map_size(total_size)
            .open(&root.join(STORAGE_DB_FILENAME))?;

        let block_header_db = env.create_db(Some("block_header"), DatabaseFlags::empty())?;
        let block_metadata_db = env.create_db(Some("block_metadata"), DatabaseFlags::empty())?;
        let deploy_db = env.create_db(Some("deploys"), DatabaseFlags::empty())?;
        let deploy_metadata_db = env.create_db(Some("deploy_metadata"), DatabaseFlags::empty())?;
        let transfer_db = env.create_db(Some("transfer"), DatabaseFlags::empty())?;
        let state_store_db = env.create_db(Some("state_store"), DatabaseFlags::empty())?;
        let block_body_v1_db = env.create_db(Some("block_body"), DatabaseFlags::empty())?;
        let block_body_v2_db = env.create_db(Some("block_body_merkle"), DatabaseFlags::empty())?;
        let deploy_hashes_db = env.create_db(Some("deploy_hashes"), DatabaseFlags::empty())?;
        let transfer_hashes_db = env.create_db(Some("transfer_hashes"), DatabaseFlags::empty())?;
        let proposer_db = env.create_db(Some("proposers"), DatabaseFlags::empty())?;

        // We now need to restore the block-height index. Log messages allow timing here.
        info!("reindexing block store");
        let mut block_height_index = BTreeMap::new();
        let mut switch_block_era_id_index = BTreeMap::new();
        let mut deploy_hash_index = BTreeMap::new();
        let mut block_txn = env.begin_rw_txn()?;
        let mut cursor = block_txn.open_rw_cursor(block_header_db)?;

        let mut deleted_block_hashes = HashSet::new();
        let mut deleted_block_body_hashes_v1 = HashSet::new();
        // Note: `iter_start` has an undocumented panic if called on an empty database. We rely on
        //       the iterator being at the start when created.
        for (raw_key, raw_val) in cursor.iter() {
            let block_header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
            if let Some(invalid_era) = hard_reset_to_start_of_era {
                // Remove blocks that are in to-be-upgraded eras, but have obsolete protocol
                // versions - they were most likely created before the upgrade and should be
                // reverted.
                if block_header.era_id() >= invalid_era
                    && block_header.protocol_version() < protocol_version
                {
                    if block_header.hashing_algorithm_version() == HashingAlgorithmVersion::V1 {
                        let _ = deleted_block_body_hashes_v1.insert(*block_header.body_hash());
                    }
                    let _ = deleted_block_hashes.insert(block_header.hash());
                    cursor.del(WriteFlags::empty())?;
                    continue;
                }
            }

            if should_check_integrity {
                let found_block_header_hash = block_header.hash();
                if raw_key != found_block_header_hash.as_ref() {
                    return Err(Error::BlockHeaderNotStoredUnderItsHash {
                        queried_block_hash_bytes: raw_key.to_vec(),
                        found_block_header_hash,
                        block_header: Box::new(block_header),
                    });
                }
            }

            insert_to_block_header_indices(
                &mut block_height_index,
                &mut switch_block_era_id_index,
                &block_header,
            )?;

            let mut body_txn = env.begin_ro_txn()?;
            let block_body: BlockBody = match block_header.hashing_algorithm_version() {
                HashingAlgorithmVersion::V1 => body_txn
                    .get_value(block_body_v1_db, block_header.body_hash())?
                    .ok_or_else(|| {
                        Error::NonExistentBlockBodyReferredToByHeader(Box::new(
                            block_header.clone(),
                        ))
                    })?,
                HashingAlgorithmVersion::V2 => get_single_block_body_v2(
                    &mut body_txn,
                    block_body_v2_db,
                    deploy_hashes_db,
                    transfer_hashes_db,
                    proposer_db,
                    block_header.body_hash(),
                )?
                .ok_or_else(|| {
                    Error::NonExistentBlockBodyReferredToByHeader(Box::new(block_header.clone()))
                })?,
            };

            if should_check_integrity {
                Block::new_from_header_and_body(block_header.clone(), block_body.clone())?;
            }

            insert_to_deploy_index(&mut deploy_hash_index, block_header.hash(), &block_body)?;
        }
        info!("block store reindexing complete");
        drop(cursor);
        block_txn.commit()?;

        let deleted_block_hashes_raw = deleted_block_hashes.iter().map(BlockHash::as_ref).collect();

        initialize_block_body_v1_db(
            &env,
            &block_header_db,
            &block_body_v1_db,
            &deleted_block_body_hashes_v1
                .iter()
                .map(Digest::as_ref)
                .collect(),
            should_check_integrity,
        )?;
        initialize_block_body_v2_db(
            &env,
            &block_header_db,
            &block_body_v2_db,
            &deploy_hashes_db,
            &transfer_hashes_db,
            &proposer_db,
            should_check_integrity,
        )?;
        initialize_block_metadata_db(
            &env,
            &block_metadata_db,
            &deleted_block_hashes_raw,
            should_check_integrity,
        )?;
        initialize_deploy_metadata_db(&env, &deploy_metadata_db, &deleted_block_hashes)?;

        Ok(Storage {
            root,
            env,
            block_header_db,
            block_body_v1_db,
            block_body_v2_db,
            deploy_hashes_db,
            transfer_hashes_db,
            proposer_db,
            block_metadata_db,
            deploy_db,
            deploy_metadata_db,
            transfer_db,
            state_store_db,
            block_height_index,
            switch_block_era_id_index,
            deploy_hash_index,
            enable_mem_deduplication: config.enable_mem_deduplication,
            deploy_cache: BlobCache::new(config.mem_pool_prune_interval),
        })
    }

    /// Handles a state store request.
    fn handle_state_store_request<REv>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        req: StateStoreRequest,
    ) -> Result<Effects<Event>, Error>
    where
        Self: Component<REv>,
    {
        // Incoming requests are fairly simple database write. Errors are handled one level above on
        // the call stack, so all we have to do is load or store a value.
        match req {
            StateStoreRequest::Save {
                key,
                data,
                responder,
            } => {
                let mut txn = self.env.begin_rw_txn()?;
                txn.put(self.state_store_db, &key, &data, WriteFlags::default())?;
                txn.commit()?;
                Ok(responder.respond(()).ignore())
            }
            #[cfg(test)]
            StateStoreRequest::Load { key, responder } => {
                let txn = self.env.begin_ro_txn()?;
                let bytes = match txn.get(self.state_store_db, &key) {
                    Ok(slice) => Some(slice.to_owned()),
                    Err(lmdb::Error::NotFound) => None,
                    Err(err) => return Err(err.into()),
                };
                Ok(responder.respond(bytes).ignore())
            }
        }
    }

    /// Reads from the state storage DB.
    /// If key is non-empty, returns bytes from under the key. Otherwise returns `Ok(None)`.
    /// May also fail with storage errors.
    pub(crate) fn read_state_store<K>(&self, key: &K) -> Result<Option<Vec<u8>>, Error>
    where
        K: AsRef<[u8]>,
    {
        let txn = self.env.begin_ro_txn()?;
        let bytes = match txn.get(self.state_store_db, &key) {
            Ok(slice) => Some(slice.to_owned()),
            Err(lmdb::Error::NotFound) => None,
            Err(err) => return Err(err.into()),
        };
        Ok(bytes)
    }

    /// Deletes value living under the key from the state storage DB.
    pub(crate) fn del_state_store<K>(&self, key: K) -> Result<bool, Error>
    where
        K: AsRef<[u8]>,
    {
        let mut txn = self.env.begin_rw_txn()?;
        let result = match txn.del(self.state_store_db, &key, None) {
            Ok(_) => Ok(true),
            Err(lmdb::Error::NotFound) => Ok(false),
            Err(err) => Err(err),
        }?;
        txn.commit()?;
        Ok(result)
    }

    /// Returns the path to the storage folder.
    pub(crate) fn root_path(&self) -> &Path {
        &self.root
    }

    /// Handles a storage request.
    fn handle_storage_request<REv>(&mut self, req: StorageRequest) -> Result<Effects<Event>, Error>
    where
        Self: Component<REv>,
    {
        // Note: Database IO is handled in a blocking fashion on purpose throughout this function.
        // The rationale is that long IO operations are very rare and cache misses frequent, so on
        // average the actual execution time will be very low.
        Ok(match req {
            StorageRequest::PutBlock { block, responder } => {
                responder.respond(self.write_block(&*block)?).ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => responder.respond(self.read_block(&block_hash)?).ignore(),
            StorageRequest::GetBlockHeaderAtHeight { height, responder } => responder
                .respond(self.get_block_header_by_height(&mut self.env.begin_ro_txn()?, height)?)
                .ignore(),
            StorageRequest::GetBlockAtHeight { height, responder } => responder
                .respond(self.get_block_by_height(&mut self.env.begin_ro_txn()?, height)?)
                .ignore(),
            StorageRequest::GetHighestBlock { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_highest_block(&mut txn)?)
                    .ignore()
            }
            StorageRequest::GetSwitchBlockHeaderAtEraId { era_id, responder } => responder
                .respond(
                    self.get_switch_block_header_by_era_id(&mut self.env.begin_ro_txn()?, era_id)?,
                )
                .ignore(),
            StorageRequest::GetBlockHeaderForDeploy {
                deploy_hash,
                responder,
            } => {
                responder
                    .respond(self.get_block_header_by_deploy_hash(
                        &mut self.env.begin_ro_txn()?,
                        deploy_hash,
                    )?)
                    .ignore()
            }
            StorageRequest::GetBlockHeader {
                block_hash,
                responder,
            } => responder
                .respond(self.get_single_block_header(&mut self.env.begin_ro_txn()?, &block_hash)?)
                .ignore(),
            StorageRequest::GetBlockTransfers {
                block_hash,
                responder,
            } => responder
                .respond(self.get_transfers(&mut self.env.begin_ro_txn()?, &block_hash)?)
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
            StorageRequest::PutExecutionResults {
                block_hash,
                execution_results,
                responder,
            } => {
                let mut txn = self.env.begin_rw_txn()?;

                let mut transfers: Vec<Transfer> = vec![];

                for (deploy_hash, execution_result) in execution_results {
                    let mut metadata = self
                        .get_deploy_metadata(&mut txn, &deploy_hash)?
                        .unwrap_or_default();

                    // If we have a previous execution result, we can continue if it is the same.
                    if let Some(prev) = metadata.execution_results.get(&block_hash) {
                        if prev == &execution_result {
                            continue;
                        } else {
                            debug!(%deploy_hash, %block_hash, "different execution result");
                        }
                    }

                    if let ExecutionResult::Success { effect, .. } = execution_result.clone() {
                        for transform_entry in effect.transforms {
                            if let Transform::WriteTransfer(transfer) = transform_entry.transform {
                                transfers.push(transfer);
                            }
                        }
                    }

                    // TODO: this is currently done like this because rpc get_deploy returns the
                    // data, but the organization of deploy, block_hash, and
                    // execution_result is incorrectly represented. it should be
                    // inverted; for a given block_hash 0n deploys and each deploy has exactly 1
                    // result (aka deploy_metadata in this context).

                    // Update metadata and write back to db.
                    metadata
                        .execution_results
                        .insert(*block_hash, execution_result);
                    let was_written =
                        txn.put_value(self.deploy_metadata_db, &deploy_hash, &metadata, true)?;
                    if !was_written {
                        error!(?block_hash, ?deploy_hash, "failed to write deploy metadata");
                        debug_assert!(was_written);
                    }
                }

                let was_written =
                    txn.put_value(self.transfer_db, &*block_hash, &transfers, true)?;
                if !was_written {
                    error!(?block_hash, "failed to write transfers");
                    debug_assert!(was_written);
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
            StorageRequest::GetBlockAndMetadataByHash {
                block_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;

                let block: Block =
                    if let Some(block) = self.get_single_block(&mut txn, &block_hash)? {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    };
                // Check that the hash of the block retrieved is correct.
                assert_eq!(&block_hash, block.hash());
                let signatures = match self.get_finality_signatures(&mut txn, &block_hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(block_hash, block.header().era_id()),
                };
                assert!(signatures.verify().is_ok());

                responder.respond(Some((block, signatures))).ignore()
            }
            StorageRequest::GetBlockAndMetadataByHeight {
                block_height,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;

                let block: Block =
                    if let Some(block) = self.get_block_by_height(&mut txn, block_height)? {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    };

                let hash = block.hash();
                let signatures = match self.get_finality_signatures(&mut txn, hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(*hash, block.header().era_id()),
                };
                responder.respond(Some((block, signatures))).ignore()
            }
            StorageRequest::GetHighestBlockWithMetadata { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let highest_block: Block = if let Some(block) = self
                    .block_height_index
                    .keys()
                    .last()
                    .and_then(|&height| self.get_block_by_height(&mut txn, height).transpose())
                    .transpose()?
                {
                    block
                } else {
                    return Ok(responder.respond(None).ignore());
                };
                let hash = highest_block.hash();
                let signatures = match self.get_finality_signatures(&mut txn, hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(*hash, highest_block.header().era_id()),
                };
                responder
                    .respond(Some((highest_block, signatures)))
                    .ignore()
            }
            StorageRequest::PutBlockSignatures {
                signatures,
                responder,
            } => {
                let mut txn = self.env.begin_rw_txn()?;
                let old_data: Option<BlockSignatures> =
                    txn.get_value(self.block_metadata_db, &signatures.block_hash)?;
                let new_data = match old_data {
                    None => signatures,
                    Some(mut data) => {
                        for (pk, sig) in signatures.proofs {
                            data.insert_proof(pk, sig);
                        }
                        data
                    }
                };
                let outcome = txn.put_value(
                    self.block_metadata_db,
                    &new_data.block_hash,
                    &new_data,
                    true,
                )?;
                txn.commit()?;
                responder.respond(outcome).ignore()
            }
            StorageRequest::GetBlockSignatures {
                block_hash,
                responder,
            } => {
                let result =
                    self.get_finality_signatures(&mut self.env.begin_ro_txn()?, &block_hash)?;
                responder.respond(result).ignore()
            }
            StorageRequest::GetFinalizedDeploys { ttl, responder } => {
                responder.respond(self.get_finalized_deploys(ttl)?).ignore()
            }
        })
    }

    /// Retrieves single block header by height by looking it up in the index and returning it.
    fn get_block_header_and_metadata_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, Error> {
        let block_hash = match self.block_height_index.get(&height) {
            None => return Ok(None),
            Some(block_hash) => block_hash,
        };
        let block_header = match self.get_single_block_header(tx, block_hash)? {
            None => return Ok(None),
            Some(block_header) => block_header,
        };
        let block_signatures = match self.get_finality_signatures(tx, block_hash)? {
            None => BlockSignatures::new(*block_hash, block_header.era_id()),
            Some(signatures) => signatures,
        };
        Ok(Some(BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        }))
    }

    /// Retrieves a block by hash.
    fn read_block(&self, block_hash: &BlockHash) -> Result<Option<Block>, Error> {
        self.get_single_block(&mut self.env.begin_ro_txn()?, block_hash)
    }

    /// Writes a block to storage, updating indices as necessary
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write_block(&mut self, block: &Block) -> Result<bool, Error> {
        // Validate the block prior to inserting it into the database
        block.verify()?;
        let mut txn = self.env.begin_rw_txn()?;
        // Write the block body
        {
            let block_body_hash = block.header().body_hash();
            let block_body = block.body();
            let success = match block.header().hashing_algorithm_version() {
                HashingAlgorithmVersion::V1 => {
                    self.put_single_block_body_v1(&mut txn, block_body_hash, block_body)?
                }
                HashingAlgorithmVersion::V2 => {
                    self.put_single_block_body_v2(&mut txn, block_body)?
                }
            };
            if !success {
                error!("Could not insert body for: {}", block);
                txn.abort();
                return Ok(false);
            }
        }

        if !txn.put_value(self.block_header_db, block.hash(), block.header(), true)? {
            error!("Could not insert block header for block: {}", block);
            txn.abort();
            return Ok(false);
        }
        insert_to_block_header_indices(
            &mut self.block_height_index,
            &mut self.switch_block_era_id_index,
            block.header(),
        )?;
        insert_to_deploy_index(
            &mut self.deploy_hash_index,
            block.header().hash(),
            block.body(),
        )?;
        txn.commit()?;
        Ok(true)
    }

    /// Retrieves a block header to handle a network request.
    pub(crate) fn read_block_header_and_finality_signatures_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, Error> {
        let mut txn = self.env.begin_ro_txn()?;
        let maybe_block_header_and_finality_signatures =
            self.get_block_header_and_metadata_by_height(&mut txn, height)?;
        drop(txn);
        Ok(maybe_block_header_and_finality_signatures)
    }

    /// Retrieves single block header by height by looking it up in the index and returning it.
    fn get_block_header_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        height: u64,
    ) -> Result<Option<BlockHeader>, Error> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block_header(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    fn get_block_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        height: u64,
    ) -> Result<Option<Block>, Error> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves single switch block header by era ID by looking it up in the index and returning
    /// it.
    fn get_switch_block_header_by_era_id<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, Error> {
        debug!(switch_block_era_id_index = ?self.switch_block_era_id_index);
        self.switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| self.get_single_block_header(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves a single block header by deploy hash by looking it up in the index and returning
    /// it.
    fn get_block_header_by_deploy_hash<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        deploy_hash: DeployHash,
    ) -> Result<Option<BlockHeader>, Error> {
        self.deploy_hash_index
            .get(&deploy_hash)
            .and_then(|block_hash| self.get_single_block_header(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves the highest block from the storage, if one exists.
    /// May return an LMDB error.
    fn get_highest_block<Tx: Transaction>(&self, txn: &mut Tx) -> Result<Option<Block>, Error> {
        self.block_height_index
            .keys()
            .last()
            .and_then(|&height| self.get_block_by_height(txn, height).transpose())
            .transpose()
    }

    /// Returns vector blocks that satisfy the predicate, starting from the latest one and following
    /// the ancestry chain.
    fn get_blocks_while<F, Tx: Transaction>(
        &self,
        txn: &mut Tx,
        predicate: F,
    ) -> Result<Vec<Block>, Error>
    where
        F: Fn(&Block) -> bool,
    {
        let mut next_block = self.get_highest_block(txn)?;
        let mut blocks = Vec::new();
        loop {
            match next_block {
                None => break,
                Some(block) if !predicate(&block) => break,
                Some(block) => {
                    next_block = match block.parent() {
                        None => None,
                        Some(parent_hash) => self.get_single_block(txn, parent_hash)?,
                    };
                    blocks.push(block);
                }
            }
        }
        Ok(blocks)
    }

    /// Returns the vector of deploys whose TTL hasn't expired yet.
    fn get_finalized_deploys(
        &self,
        ttl: TimeDiff,
    ) -> Result<Vec<(DeployHash, DeployHeader)>, Error> {
        let mut txn = self.env.begin_ro_txn()?;
        // We're interested in deploys whose TTL hasn't expired yet.
        let ttl_expired = |block: &Block| block.timestamp().elapsed() < ttl;
        let mut deploys = Vec::new();
        for block in self.get_blocks_while(&mut txn, ttl_expired)? {
            for deploy_hash in block
                .body()
                .deploy_hashes()
                .iter()
                .chain(block.body().transfer_hashes())
            {
                let deploy_header = match self.get_deploy_header(&mut txn, deploy_hash)? {
                    Some(deploy_header) => deploy_header,
                    None => {
                        let deploy_hash = deploy_hash.to_owned();
                        return Err(Error::NonExistentDeploy {
                            block: Box::new(block),
                            deploy_hash,
                        });
                    }
                };
                // If block's deploy has already expired, ignore it.
                // It may happen that deploy was not expired at the time of proposing a block but it
                // is now.
                if deploy_header.timestamp().elapsed() > ttl {
                    continue;
                }
                deploys.push((*deploy_hash, deploy_header));
            }
        }
        Ok(deploys)
    }

    /// Retrieves the state root hashes from storage to check the integrity of the trie store.
    pub(crate) fn read_state_root_hashes_for_trie_check(&self) -> Result<Vec<Digest>, Error> {
        let mut blake_hashes: Vec<Digest> = Vec::new();
        let txn = self.env.begin_ro_txn()?;
        let mut cursor = txn.open_ro_cursor(self.block_header_db)?;
        for (_, raw_val) in cursor.iter() {
            let header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
            let blake_hash = header.state_root_hash();
            blake_hashes.push(*blake_hash);
        }

        blake_hashes.sort();
        blake_hashes.dedup();

        Ok(blake_hashes)
    }

    /// Retrieves a single block header in a separate transaction from storage.
    fn get_single_block_header<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, Error> {
        let block_header: BlockHeader = match tx.get_value(self.block_header_db, &block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let found_block_header_hash = block_header.hash();
        if found_block_header_hash != *block_hash {
            return Err(Error::BlockHeaderNotStoredUnderItsHash {
                queried_block_hash_bytes: block_hash.as_ref().to_vec(),
                found_block_header_hash,
                block_header: Box::new(block_header),
            });
        };
        Ok(Some(block_header))
    }

    /// Retrieves a single Merklized block body in a separate transaction from storage.
    fn get_single_block_body_v2<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_body_hash: &Digest,
    ) -> Result<Option<BlockBody>, LmdbExtError> {
        get_single_block_body_v2(
            tx,
            self.block_body_v2_db,
            self.deploy_hashes_db,
            self.transfer_hashes_db,
            self.proposer_db,
            block_body_hash,
        )
    }

    /// Writes a single block body in a separate transaction to storage.
    fn put_single_block_body_v1(
        &self,
        tx: &mut RwTransaction,
        block_body_hash: &Digest,
        block_body: &BlockBody,
    ) -> Result<bool, LmdbExtError> {
        tx.put_value(self.block_body_v1_db, block_body_hash, block_body, true)
            .map_err(Into::into)
    }

    fn put_merkle_block_body_part<'a, T>(
        &self,
        tx: &mut RwTransaction,
        part_database: Database,
        merklized_block_body_part: &MerkleBlockBodyPart<'a, T>,
    ) -> Result<bool, LmdbExtError>
    where
        T: ToBytes,
    {
        // It's possible the value is already present (ie, if it is a block proposer).
        // We put the value and rest hashes in first, since we need that present even if the value
        // is already there.
        if !tx.put_value_bytesrepr(
            self.block_body_v2_db,
            merklized_block_body_part.merkle_linked_list_node_hash(),
            &merklized_block_body_part.value_and_rest_hashes_pair(),
            true,
        )? {
            return Ok(false);
        };

        if !tx.put_value_bytesrepr(
            part_database,
            merklized_block_body_part.value_hash(),
            merklized_block_body_part.value(),
            true,
        )? {
            return Ok(false);
        };
        Ok(true)
    }

    /// Writes a single merklized block body in a separate transaction to storage.
    fn put_single_block_body_v2(
        &self,
        tx: &mut RwTransaction,
        block_body: &BlockBody,
    ) -> Result<bool, LmdbExtError> {
        let merkle_block_body = block_body.merklize();
        let MerkleBlockBody {
            deploy_hashes,
            transfer_hashes,
            proposer,
        } = &merkle_block_body;
        if !self.put_merkle_block_body_part(tx, self.deploy_hashes_db, deploy_hashes)? {
            return Ok(false);
        };
        if !self.put_merkle_block_body_part(tx, self.transfer_hashes_db, transfer_hashes)? {
            return Ok(false);
        };
        if !self.put_merkle_block_body_part(tx, self.proposer_db, proposer)? {
            return Ok(false);
        };
        Ok(true)
    }

    // Retrieves a block header by hash.
    pub(crate) fn get_block_header_by_hash(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, Error> {
        let mut txn = self.env.begin_ro_txn()?;
        let maybe_block_header = self.get_single_block_header(&mut txn, block_hash)?;
        drop(txn);
        Ok(maybe_block_header)
    }

    /// Retrieves a single block in a separate transaction from storage.
    fn get_single_block<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Block>, Error> {
        let block_header: BlockHeader = match self.get_single_block_header(tx, block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let maybe_block_body = self.get_body_for_block_header(tx, &block_header)?;
        let block_body = match maybe_block_body {
            Some(block_body) => block_body,
            None => {
                warn!(
                    ?block_header,
                    "retrieved block header but block body is missing from database"
                );
                return Ok(None);
            }
        };
        let block = Block::new_from_header_and_body(block_header, block_body)?;
        Ok(Some(block))
    }

    fn get_body_for_block_header<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_header: &BlockHeader,
    ) -> Result<Option<BlockBody>, LmdbExtError> {
        match block_header.hashing_algorithm_version() {
            HashingAlgorithmVersion::V1 => {
                self.get_single_block_body_v1(tx, block_header.body_hash())
            }
            HashingAlgorithmVersion::V2 => {
                self.get_single_block_body_v2(tx, block_header.body_hash())
            }
        }
    }

    fn get_single_block_body_v1<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_body_hash: &Digest,
    ) -> Result<Option<BlockBody>, LmdbExtError> {
        tx.get_value(self.block_body_v1_db, block_body_hash)
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

    /// Returns the deploy's header.
    fn get_deploy_header<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        deploy_hash: &DeployHash,
    ) -> Result<Option<DeployHeader>, LmdbExtError> {
        let maybe_deploy: Option<Deploy> = txn.get_value(self.deploy_db, deploy_hash)?;
        Ok(maybe_deploy.map(|deploy| deploy.header().clone()))
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

    /// Retrieves transfers associated with block.
    ///
    /// If no transfers are stored for the block, an empty transfers instance will be
    /// created, but not stored.
    fn get_transfers<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, Error> {
        Ok(tx.get_value(self.transfer_db, block_hash)?)
    }

    /// Retrieves finality signatures for a block with a given block hash
    fn get_finality_signatures<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, Error> {
        Ok(tx.get_value(self.block_metadata_db, block_hash)?)
    }
}

/// Inserts the relevant entries to the two indices.
///
/// If a duplicate entry is encountered, neither index is updated and an error is returned.
fn insert_to_block_header_indices(
    block_height_index: &mut BTreeMap<u64, BlockHash>,
    switch_block_era_id_index: &mut BTreeMap<EraId, BlockHash>,
    block_header: &BlockHeader,
) -> Result<(), Error> {
    let block_hash = block_header.hash();
    if let Some(first) = block_height_index.get(&block_header.height()) {
        if *first != block_hash {
            return Err(Error::DuplicateBlockIndex {
                height: block_header.height(),
                first: *first,
                second: block_hash,
            });
        }
    }

    if block_header.is_switch_block() {
        match switch_block_era_id_index.entry(block_header.era_id()) {
            Entry::Vacant(entry) => {
                let _ = entry.insert(block_hash);
            }
            Entry::Occupied(entry) => {
                if *entry.get() != block_hash {
                    return Err(Error::DuplicateEraIdIndex {
                        era_id: block_header.era_id(),
                        first: *entry.get(),
                        second: block_hash,
                    });
                }
            }
        }
    }

    let _ = block_height_index.insert(block_header.height(), block_hash);
    Ok(())
}

/// Inserts the relevant entries to the index.
///
/// If a duplicate entry is encountered, index is not updated and an error is returned.
fn insert_to_deploy_index(
    deploy_hash_index: &mut BTreeMap<DeployHash, BlockHash>,
    block_hash: BlockHash,
    block_body: &BlockBody,
) -> Result<(), Error> {
    if let Some(hash) = block_body
        .deploy_hashes()
        .iter()
        .chain(block_body.transfer_hashes().iter())
        .find(|hash| {
            deploy_hash_index
                .get(hash)
                .map_or(false, |old_block_hash| *old_block_hash != block_hash)
        })
    {
        return Err(Error::DuplicateDeployIndex {
            deploy_hash: *hash,
            first: deploy_hash_index[hash],
            second: block_hash,
        });
    }

    for hash in block_body
        .deploy_hashes()
        .iter()
        .chain(block_body.transfer_hashes().iter())
    {
        deploy_hash_index.insert(*hash, block_hash);
    }

    Ok(())
}

fn should_move_storage_files_to_network_subdir(
    root: &Path,
    file_names: &[&str],
) -> Result<bool, Error> {
    let mut files_found = vec![];
    let mut files_not_found = vec![];

    file_names.iter().for_each(|file_name| {
        let file_path = root.join(file_name);

        match file_path.exists() {
            true => files_found.push(file_path),
            false => files_not_found.push(file_path),
        }
    });

    let should_move_files = files_found.len() == file_names.len();

    if !should_move_files && !files_found.is_empty() {
        error!(
            "found storage files: {:?}, missing storage files: {:?}",
            files_found, files_not_found
        );

        return Err(Error::MissingStorageFiles {
            missing_files: files_not_found,
        });
    }

    Ok(should_move_files)
}

fn move_storage_files_to_network_subdir(
    root: &Path,
    subdir: &Path,
    file_names: &[&str],
) -> Result<(), Error> {
    file_names
        .iter()
        .map(|file_name| {
            let source_path = root.join(file_name);
            let dest_path = subdir.join(file_name);
            fs::rename(&source_path, &dest_path).map_err(|original_error| Error::UnableToMoveFile {
                source_path,
                dest_path,
                original_error,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    info!(
        "moved files: {:?} from: {:?} to: {:?}",
        file_names, root, subdir
    );
    Ok(())
}

/// On-disk storage configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
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
    /// The maximum size of the database to use for the component state store.
    ///
    /// The size should be a multiple of the OS page size.
    max_state_store_size: usize,
    /// Whether or not memory deduplication is enabled.
    enable_mem_deduplication: bool,
    /// How many loads before memory duplication checks for dead references.
    mem_pool_prune_interval: u16,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // No one should be instantiating a config with storage set to default.
            path: "/dev/null".into(),
            max_block_store_size: DEFAULT_MAX_BLOCK_STORE_SIZE,
            max_deploy_store_size: DEFAULT_MAX_DEPLOY_STORE_SIZE,
            max_deploy_metadata_store_size: DEFAULT_MAX_DEPLOY_METADATA_STORE_SIZE,
            max_state_store_size: DEFAULT_MAX_STATE_STORE_SIZE,
            enable_mem_deduplication: false,
            mem_pool_prune_interval: 1024,
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
            Event::StateStoreRequest(req) => req.fmt(f),
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
    /// Retrieves a deploy from the deploy store to handle a legacy network request.
    fn handle_legacy_direct_deploy_request(&self, deploy_hash: DeployHash) -> Option<Deploy> {
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

    /// Retrieves a potentially deduplicated deploy from the deploy store or cache, depending on
    /// runtime configuration.
    pub(crate) fn handle_deduplicated_legacy_direct_deploy_request(
        &mut self,
        deploy_hash: DeployHash,
    ) -> Option<SharedObject<Vec<u8>>> {
        if self.enable_mem_deduplication {
            // On a cache hit, return directly from the cache.
            if let Some(serialized) = self.deploy_cache.get(&deploy_hash) {
                return Some(SharedObject::shared(serialized));
            }
        }

        let deploy = self.handle_legacy_direct_deploy_request(deploy_hash)?;

        match bincode::serialize(&deploy) {
            Ok(serialized) => {
                if self.enable_mem_deduplication {
                    // We found a deploy, ensure it gets added to the cache.
                    let arc = Arc::new(serialized);
                    self.deploy_cache.put(deploy_hash, Arc::downgrade(&arc));
                    Some(SharedObject::shared(arc))
                } else {
                    Some(SharedObject::owned(serialized))
                }
            }
            Err(err) => {
                // We failed to serialize a stored deploy, which is a serious issue. There is no
                // good recovery from this, so we just pretend we don't have it, logging an error.
                error!(
                    err = display_error(&err),
                    "failed to create (shared) get-response"
                );
                None
            }
        }
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
    pub(crate) fn get_deploy_by_hash(&self, deploy_hash: DeployHash) -> Option<Deploy> {
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
    pub(crate) fn get_all_deploy_hashes(&self) -> BTreeSet<DeployHash> {
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

    /// Retrieves single switch block by era ID by looking it up in the index and returning it.
    fn get_switch_block_by_era_id<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<Block>, Error> {
        self.switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| self.get_single_block(tx, block_hash).transpose())
            .transpose()
    }

    /// Get the switch block for a specified era number in a read-only LMDB database transaction.
    ///
    /// # Panics
    ///
    /// Panics on any IO or db corruption error.
    pub(crate) fn transactional_get_switch_block_by_era_id(
        &self,
        switch_block_era_num: u64,
    ) -> Option<Block> {
        let mut read_only_lmdb_transaction = self
            .env
            .begin_ro_txn()
            .expect("Could not start read only transaction for lmdb");
        let switch_block = self
            .get_switch_block_by_era_id(
                &mut read_only_lmdb_transaction,
                EraId::from(switch_block_era_num),
            )
            .expect("LMDB panicked trying to get switch block");
        read_only_lmdb_transaction
            .commit()
            .expect("Could not commit transaction");
        switch_block
    }
}

fn construct_block_body_to_block_header_reverse_lookup(
    tx: &impl Transaction,
    block_header_db: &Database,
) -> Result<BTreeMap<Digest, BlockHeader>, LmdbExtError> {
    let mut block_body_hash_to_header_map: BTreeMap<Digest, BlockHeader> = BTreeMap::new();
    for (_raw_key, raw_val) in tx.open_ro_cursor(*block_header_db)?.iter() {
        let block_header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
        block_body_hash_to_header_map.insert(block_header.body_hash().to_owned(), block_header);
    }
    Ok(block_body_hash_to_header_map)
}

/// Purges stale entries from the block body database, and checks the integrity of the remainder if
/// `should_check_integrity` is true.
fn initialize_block_body_v1_db(
    env: &Environment,
    block_header_db: &Database,
    block_body_v1_db: &Database,
    deleted_block_body_hashes_raw: &HashSet<&[u8]>,
    should_check_integrity: bool,
) -> Result<(), Error> {
    info!("initializing v1 block body database");
    let mut txn = env.begin_rw_txn()?;

    let block_body_hash_to_header_map =
        construct_block_body_to_block_header_reverse_lookup(&txn, block_header_db)?;

    let mut cursor = txn.open_rw_cursor(*block_body_v1_db)?;

    for (raw_key, _raw_val) in cursor.iter() {
        let block_body_hash =
            Digest::try_from(raw_key).map_err(|err| LmdbExtError::DataCorrupted(Box::new(err)))?;
        if !block_body_hash_to_header_map.contains_key(&block_body_hash) {
            if !deleted_block_body_hashes_raw.contains(raw_key) {
                // This means that the block body isn't referenced by any header, but no header
                // referencing it was just deleted, either
                warn!(?raw_key, "orphaned block body detected");
            }
            info!(?raw_key, "deleting v1 block body");
            cursor.del(WriteFlags::empty())?;
        }
    }

    drop(cursor);

    if should_check_integrity {
        let expected_hashing_algorithm_version = HashingAlgorithmVersion::V1;
        for (raw_key, raw_val) in txn.open_ro_cursor(*block_body_v1_db)?.iter() {
            let block_body_hash = Digest::try_from(raw_key)
                .map_err(|err| LmdbExtError::DataCorrupted(Box::new(err)))?;
            let block_body: BlockBody = lmdb_ext::deserialize(raw_val)?;
            if let Some(block_header) = block_body_hash_to_header_map.get(&block_body_hash) {
                let actual_hashing_algorithm_version = block_header.hashing_algorithm_version();
                if expected_hashing_algorithm_version != actual_hashing_algorithm_version {
                    return Err(Error::UnexpectedHashingAlgorithmVersion {
                        expected_hashing_algorithm_version,
                        actual_hashing_algorithm_version,
                    });
                }
                // Use smart constructor for block and propagate validation error accordingly
                Block::new_from_header_and_body(block_header.to_owned(), block_body)?;
            } else {
                // Should be unreachable because we just deleted all block bodies that aren't
                // referenced by any header
                return Err(Error::NoBlockHeaderForBlockBody {
                    block_body_hash,
                    hashing_algorithm_version: expected_hashing_algorithm_version,
                    block_body: Box::new(block_body),
                });
            }
        }
    }

    txn.commit()?;
    info!("v1 block body database initialized");
    Ok(())
}

fn get_merkle_linked_list_node<Tx, T>(
    tx: &mut Tx,
    block_body_v2_db: Database,
    part_database: Database,
    key_to_block_body_db: &Digest,
) -> Result<Option<MerkleLinkedListNode<T>>, LmdbExtError>
where
    Tx: Transaction,
    T: FromBytes,
{
    let (part_to_value_db, merkle_proof_of_rest): (Digest, Digest) =
        match tx.get_value_bytesrepr(block_body_v2_db, key_to_block_body_db)? {
            Some(slice) => slice,
            None => return Ok(None),
        };
    let value = match tx.get_value_bytesrepr(part_database, &part_to_value_db)? {
        Some(value) => value,
        None => return Ok(None),
    };
    Ok(Some(MerkleLinkedListNode::new(value, merkle_proof_of_rest)))
}

/// Retrieves a single Merklized block body in a separate transaction from storage.
fn get_single_block_body_v2<Tx: Transaction>(
    tx: &mut Tx,
    block_body_v2_db: Database,
    deploy_hashes_db: Database,
    transfer_hashes_db: Database,
    proposer_db: Database,
    block_body_hash: &Digest,
) -> Result<Option<BlockBody>, LmdbExtError> {
    let deploy_hashes_with_proof: MerkleLinkedListNode<Vec<DeployHash>> =
        match get_merkle_linked_list_node(tx, block_body_v2_db, deploy_hashes_db, block_body_hash)?
        {
            Some(deploy_hashes_with_proof) => deploy_hashes_with_proof,
            None => return Ok(None),
        };
    let transfer_hashes_with_proof: MerkleLinkedListNode<Vec<DeployHash>> =
        match get_merkle_linked_list_node(
            tx,
            block_body_v2_db,
            transfer_hashes_db,
            deploy_hashes_with_proof.merkle_proof_of_rest(),
        )? {
            Some(transfer_hashes_with_proof) => transfer_hashes_with_proof,
            None => return Ok(None),
        };
    let proposer_with_proof: MerkleLinkedListNode<PublicKey> = match get_merkle_linked_list_node(
        tx,
        block_body_v2_db,
        proposer_db,
        transfer_hashes_with_proof.merkle_proof_of_rest(),
    )? {
        Some(proposer_with_proof) => {
            debug_assert_eq!(*proposer_with_proof.merkle_proof_of_rest(), hash::SENTINEL1);
            proposer_with_proof
        }
        None => return Ok(None),
    };
    let block_body = BlockBody::new(
        proposer_with_proof.take_value(),
        deploy_hashes_with_proof.take_value(),
        transfer_hashes_with_proof.take_value(),
    );

    Ok(Some(block_body))
}

// TODO: remove once we run the garbage collection again
#[allow(dead_code)]
fn garbage_collect_block_body_v2_db(
    txn: &mut RwTransaction,
    block_body_v2_db: &Database,
    deploy_hashes_db: &Database,
    transfer_hashes_db: &Database,
    proposer_db: &Database,
    block_body_hash_to_header_map: &BTreeMap<Digest, BlockHeader>,
) -> Result<(), Error> {
    // This will store all the keys that are reachable from a block header, in all the parts
    // databases (we're basically doing a mark-and-sweep below).
    // The entries correspond to: the block_body_v2_db, deploy_hashes_db, transfer_hashes_db,
    // proposer_db respectively.
    let mut live_digests: [HashSet<Digest>; BlockBody::PARTS_COUNT + 1] = [
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
    ];

    for (body_hash, header) in block_body_hash_to_header_map {
        if header.hashing_algorithm_version() != HashingAlgorithmVersion::V2 {
            // We're only interested in body hashes for V2 blocks here
            continue;
        }
        let mut current_digest = *body_hash;
        let mut live_digests_index = 1;
        while current_digest != hash::SENTINEL1 && !live_digests[0].contains(&current_digest) {
            live_digests[0].insert(current_digest);
            let (key_to_part_db, merkle_proof_of_rest): (Digest, Digest) =
                match txn.get_value_bytesrepr(*block_body_v2_db, &current_digest)? {
                    Some(slice) => slice,
                    None => {
                        return Err(Error::CouldNotFindBlockBodyPart {
                            block_hash: header.hash(),
                            merkle_linked_list_node_hash: current_digest,
                        })
                    }
                };
            if live_digests_index < live_digests.len() {
                live_digests[live_digests_index].insert(key_to_part_db);
            } else {
                return Err(Error::UnexpectedBlockBodyPart {
                    block_body_hash: *body_hash,
                    part_hash: key_to_part_db,
                });
            }
            live_digests_index += 1;
            current_digest = merkle_proof_of_rest;
        }
    }

    let databases_to_clean: [(&Database, &str); BlockBody::PARTS_COUNT + 1] = [
        (block_body_v2_db, "deleting v2 block body part"),
        (deploy_hashes_db, "deleting v2 deploy hashes entry"),
        (transfer_hashes_db, "deleting v2 transfer hashes entry"),
        (proposer_db, "deleting v2 proposer entry"),
    ];

    // Clean dead entries from all the databases
    for (index, (database, info_text)) in databases_to_clean.iter().enumerate() {
        let mut cursor = txn.open_rw_cursor(**database)?;
        for (raw_key, _raw_val) in cursor.iter() {
            let key = Digest::try_from(raw_key)
                .map_err(|err| LmdbExtError::DataCorrupted(Box::new(err)))?;
            if !live_digests[index].contains(&key) {
                info!(?raw_key, info_text);
                cursor.del(WriteFlags::empty())?;
            }
        }
        drop(cursor);
    }

    Ok(())
}

/// Purges stale entries from the (Merklized) block body database, and checks the integrity of the
/// remainder if `should_check_integrity` is true.
#[allow(clippy::too_many_arguments)]
fn initialize_block_body_v2_db(
    env: &Environment,
    block_header_db: &Database,
    block_body_v2_db: &Database,
    deploy_hashes_db: &Database,
    transfer_hashes_db: &Database,
    proposer_db: &Database,
    should_check_integrity: bool,
) -> Result<(), Error> {
    info!("initializing v2 block body database");

    if should_check_integrity {
        let txn = env.begin_rw_txn()?;

        let block_body_hash_to_header_map =
            construct_block_body_to_block_header_reverse_lookup(&txn, block_header_db)?;

        let expected_hashing_algorithm_version = HashingAlgorithmVersion::V2;
        for (raw_key, _raw_val) in txn.open_ro_cursor(*block_body_v2_db)?.iter() {
            let block_body_hash = Digest::try_from(raw_key)
                .map_err(|err| LmdbExtError::DataCorrupted(Box::new(err)))?;
            let block_header = match block_body_hash_to_header_map.get(&block_body_hash) {
                Some(block_header) => block_header,
                None => {
                    // This probably means that the key is not the hash of the whole block body, but
                    // a Merkle proof of a part of it - so we ignore this case.
                    continue;
                }
            };
            let actual_hashing_algorithm_version = block_header.hashing_algorithm_version();
            if expected_hashing_algorithm_version != actual_hashing_algorithm_version {
                return Err(Error::UnexpectedHashingAlgorithmVersion {
                    expected_hashing_algorithm_version,
                    actual_hashing_algorithm_version,
                });
            }
            // txn is unusable because it's used in the cursor - construct a temporary RO
            // transaction for reading the body
            let mut txn2 = env.begin_ro_txn()?;
            match get_single_block_body_v2(
                &mut txn2,
                *block_body_v2_db,
                *deploy_hashes_db,
                *transfer_hashes_db,
                *proposer_db,
                &block_body_hash,
            )? {
                Some(block_body) => {
                    // Use smart constructor for block and propagate validation error accordingly
                    Block::new_from_header_and_body(block_header.to_owned(), block_body)?;
                }
                None => {
                    // get_single_block_body_v2 returning an Ok(None) means we have an
                    // incomplete block body - this doesn't have to indicate an error, it may
                    // be caused by fast sync not downloading the whole body, but only a part
                    // of it - log it and skip the check
                    info!(?block_body_hash, "incomplete block body found");
                }
            };
            txn2.commit()?;
        }
        txn.commit()?;
    }

    info!("v2 block body database initialized");
    Ok(())
}

/// Purges stale entries from the block metadata database, and checks the integrity of the remainder
/// if `should_check_integrity` is true.
fn initialize_block_metadata_db(
    env: &Environment,
    block_metadata_db: &Database,
    deleted_block_hashes: &HashSet<&[u8]>,
    should_check_integrity: bool,
) -> Result<(), Error> {
    info!("initializing block metadata database");
    let mut txn = env.begin_rw_txn()?;
    let mut cursor = txn.open_rw_cursor(*block_metadata_db)?;

    for (raw_key, raw_val) in cursor.iter() {
        if deleted_block_hashes.contains(raw_key) {
            cursor.del(WriteFlags::empty())?;
            continue;
        }

        if should_check_integrity {
            let signatures: BlockSignatures = lmdb_ext::deserialize(raw_val)?;

            // Signature verification could be very slow process
            // It iterates over every signature and verifies them.
            signatures.verify().map_err(Error::SignatureVerification)?;
            if raw_key != signatures.block_hash.as_ref() {
                return Err(Error::CorruptedBlockSignatureIndex {
                    raw_key: raw_key.to_vec(),
                    block_hash_bytes: signatures.block_hash.as_ref().to_vec(),
                });
            };
        }
    }

    drop(cursor);
    txn.commit()?;

    info!("block metadata database initialized");
    Ok(())
}

/// Purges stale entries from the deploy metadata database.
fn initialize_deploy_metadata_db(
    env: &Environment,
    deploy_metadata_db: &Database,
    deleted_block_hashes: &HashSet<BlockHash>,
) -> Result<(), LmdbExtError> {
    info!("initializing deploy metadata database");
    let mut txn = env.begin_rw_txn()?;
    let mut cursor = txn.open_rw_cursor(*deploy_metadata_db)?;

    for (raw_key, raw_val) in cursor.iter() {
        let mut deploy_metadata: DeployMetadata = lmdb_ext::deserialize(raw_val)?;
        let len_before = deploy_metadata.execution_results.len();

        deploy_metadata.execution_results = deploy_metadata
            .execution_results
            .drain()
            .filter(|(block_hash, _)| !deleted_block_hashes.contains(block_hash))
            .collect();

        // If the deploy's execution results are now empty, we just remove them entirely.
        if deploy_metadata.execution_results.is_empty() {
            cursor.del(WriteFlags::empty())?;
        } else if len_before != deploy_metadata.execution_results.len() {
            let buffer = lmdb_ext::serialize(&deploy_metadata)?;
            cursor.put(&raw_key, &buffer, WriteFlags::empty())?;
        }
    }

    drop(cursor);
    txn.commit()?;

    info!("deploy metadata database initialized");
    Ok(())
}
