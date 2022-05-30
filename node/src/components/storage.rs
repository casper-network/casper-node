//! Central storage component.
//!
//! The central storage component is in charge of persisting data to disk. Its core functionalities
//! are
//!
//! * storing and loading blocks,
//! * storing and loading deploys,
//! * [temporary until refactored] holding `DeployMetadata` for each deploy,
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

mod block_getter_id;
mod block_height_index_value;
mod config;
mod error;
mod index_dbs;
mod lmdb_ext;
mod object_pool;
#[cfg(test)]
mod tests;

#[cfg(test)]
use std::collections::BTreeSet;
use std::{
    cmp,
    collections::HashMap,
    convert::TryFrom,
    fmt::{self, Display, Formatter},
    fs, mem,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use datasize::DataSize;
use derive_more::From;
use lmdb::{
    Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, RwTransaction, Transaction,
    WriteFlags,
};
use num_rational::Ratio;
use serde::Serialize;
use smallvec::SmallVec;
use static_assertions::const_assert;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    EraId, ExecutionResult, ProtocolVersion, PublicKey, TimeDiff, Transfer, Transform,
};

// The reactor! macro needs this in the fetcher tests
pub(crate) use crate::effect::requests::StorageRequest;
use crate::{
    components::{consensus, fetcher::FetchedOrNotFound, Component},
    effect::{
        incoming::{NetRequest, NetRequestIncoming},
        requests::{NetworkRequest, StateStoreRequest},
        EffectBuilder, EffectExt, Effects, Multiple,
    },
    fatal,
    protocol::Message,
    reactor::ReactorEvent,
    types::{
        AvailableBlockRange, Block, BlockAndDeploys, BlockBody, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockSignatures, BlockWithMetadata, Deploy, DeployHash,
        DeployMetadata, DeployMetadataExt, DeployWithFinalizedApprovals, FinalizedApprovals,
        FinalizedApprovalsWithId, HashingAlgorithmVersion, Item, MerkleBlockBody,
        MerkleBlockBodyPart, MerkleLinkedListNode, NodeId,
    },
    utils::{display_error, FlattenResult, WithDir},
    NodeRng,
};
use block_getter_id::BlockGetterId;
use block_height_index_value::BlockHeightIndexValue;
pub use config::Config;
pub use error::FatalStorageError;
use error::GetRequestError;
use index_dbs::IndexDbs;
use lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt};
use object_pool::ObjectPool;

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";

/// LMDB key for recording progress of populating the indices.
const PROGRESS_KEY: &str = "populating indices progress";

/// Maximum number of allowed dbs.
const MAX_DB_COUNT: u32 = 16;

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

/// Storage component.
#[derive(DataSize, Debug)]
pub struct Storage {
    /// The inner storage component.
    storage: Arc<StorageInner>,
    /// Semaphore guarding the maximum number of storage accesses running in parallel.
    sync_task_limiter: Arc<Semaphore>,
}

/// The inner storage component.
#[derive(DataSize, Debug)]
pub struct StorageInner {
    /// Storage location.
    root: PathBuf,
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    env: Environment,
    /// A map of block ID (`BlockHash`) to block header (`BlockHeader`).
    #[data_size(skip)]
    block_header_db: Database,
    /// A map of block v1 body hash (`Digest`) to block body (`BlockBody`).
    #[data_size(skip)]
    block_body_v1_db: Database,
    /// A map of block v2 (Merklized) body part (`Digest`) to a pair of hashes of the value of the
    /// block body part and the linked list node (`(Digest, Digest)`).
    #[data_size(skip)]
    block_body_v2_db: Database,
    /// A collection of block v2 deploy hashes as Merklized body parts.  The map is from body part
    /// hash (`Digest`) to set of deploy IDs (`Vec<DeployHash>`).
    #[data_size(skip)]
    deploy_hashes_db: Database,
    /// A collection of block v2 transfer hashes as Merklized body parts.  The map is from body
    /// part hash (`Digest`) to set of deploy IDs (`Vec<DeployHash>`).
    #[data_size(skip)]
    transfer_hashes_db: Database,
    /// A collection of block v2 proposers as Merklized body parts.  The map is from body part hash
    /// (`Digest`) to proposer ID (`PublicKey`).
    #[data_size(skip)]
    proposer_db: Database,
    /// A map of block ID (`BlockHash`) to set of transfers associated with the block
    /// (`Vec<Transfer>`).
    #[data_size(skip)]
    transfer_db: Database,
    /// A map of block ID (`BlockHash`) to set of block proofs (`BlockSignatures`).
    #[data_size(skip)]
    block_metadata_db: Database,
    /// A map of deploy ID (`DeployHash`) to deploy (`Deploy`).
    #[data_size(skip)]
    deploy_db: Database,
    /// A map of deploy ID (`DeployHash`) to deploy metadata (`DeployMetadata`) which is
    /// essentially the execution results.
    #[data_size(skip)]
    deploy_metadata_db: Database,
    /// A map of deploy ID (`DeployHash`) to set of finalized approvals as per included in a
    /// finalized block (`FinalizedApprovals`).
    #[data_size(skip)]
    finalized_approvals_db: Database,
    /// A general purpose database allowing components to persist arbitrary state.
    #[data_size(skip)]
    state_store_db: Database,
    /// The index databases.
    #[data_size(skip)]
    indices: IndexDbs,
    /// Highest block available when storage is constructed.
    highest_block_at_startup: u64,
    /// Whether or not memory deduplication is enabled.
    enable_mem_deduplication: bool,
    /// An in-memory pool of already loaded serialized items.
    ///
    /// Keyed by serialized item ID, contains the serialized item.
    // Note: `DataSize` is skipped here to avoid incurring locking overhead.
    #[data_size(skip)]
    serialized_item_pool: RwLock<ObjectPool<Box<[u8]>>>,
    /// The fraction of validators, by weight, that have to sign a block to prove its finality.
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
    /// The most recent era in which the network was manually restarted.
    last_emergency_restart: Option<EraId>,
    /// The era ID starting at which the new Merkle tree-based hashing scheme is applied.
    verifiable_chunked_hash_activation: EraId,
}

/// A storage component event.
#[derive(Debug, From, Serialize)]
#[repr(u8)]
pub(crate) enum Event {
    /// Storage request.
    #[from]
    StorageRequest(StorageRequest),
    /// Incoming net request.
    NetRequestIncoming(Box<NetRequestIncoming>),
    /// Incoming state storage request.
    #[from]
    StateStoreRequest(StateStoreRequest),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::StorageRequest(req) => req.fmt(f),
            Event::NetRequestIncoming(incoming) => incoming.fmt(f),
            Event::StateStoreRequest(req) => req.fmt(f),
        }
    }
}

impl From<NetRequestIncoming> for Event {
    #[inline]
    fn from(incoming: NetRequestIncoming) -> Self {
        Event::NetRequestIncoming(Box::new(incoming))
    }
}

impl<REv> Component<REv> for Storage
where
    REv: ReactorEvent + From<NetworkRequest<Message>>,
{
    type Event = Event;
    type ConstructionError = FatalStorageError;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        // We create an additional reference to the inner storage.
        let storage = self.storage.clone();
        let sync_task_limiter = self.sync_task_limiter.clone();

        async move {
            // The actual operation can now be moved into a closure that has its own independent
            // storage handle.
            let perform_op = move || {
                match event {
                    Event::StorageRequest(req) => storage.handle_storage_request::<REv>(req),
                    Event::NetRequestIncoming(ref incoming) => {
                        match storage.handle_net_request_incoming::<REv>(effect_builder, incoming) {
                            Ok(effects) => Ok(effects),
                            Err(GetRequestError::Fatal(fatal_error)) => Err(fatal_error),
                            Err(ref other_err) => {
                                warn!(
                                    sender=%incoming.sender,
                                    err=display_error(other_err),
                                    "error handling net request"
                                );
                                // We could still send the requester a "not found" message, and
                                // could do so even in the fatal case, but it is safer to not do so
                                // at the moment, giving less surface area for possible
                                // amplification attacks.
                                Ok(Effects::new())
                            }
                        }
                    }
                    Event::StateStoreRequest(req) => {
                        storage.handle_state_store_request::<REv>(effect_builder, req)
                    }
                }
            };

            // While the operation is ready to run, we do not want to spawn an arbitrarily large
            // number of synchronous background tasks, especially since tokio does not put
            // meaningful bounds on these by default. For this reason, we limit the maximum number
            // of blocking tasks using a semaphore.
            let outcome = match sync_task_limiter.acquire().await {
                Ok(_permit) => tokio::task::spawn_blocking(perform_op)
                    .await
                    .map_err(FatalStorageError::FailedToJoinBackgroundTask)
                    .flatten_result(),
                Err(err) => Err(FatalStorageError::SemaphoreClosedUnexpectedly(err)),
            };

            // On success, execute the effects (we are in an async function, so we can await them
            // directly). For errors, we crash on fatal ones, but only log non-fatal ones.
            //
            // This is almost equivalent to executing the effects, except that they are not run in
            // parallel if there are multiple.
            match outcome {
                Ok(effects) => {
                    // Run all returned effects in a single future.
                    let outputs = futures::future::join_all(effects.into_iter()).await;

                    // Flatten resulting events.
                    let events: Multiple<_> = outputs.into_iter().flatten().collect();

                    events
                }
                Err(ref err) => {
                    // Any error is turned into a fatal effect, the component itself does not panic.
                    fatal!(effect_builder, "storage error: {}", display_error(err)).await;
                    Multiple::new()
                }
            }
        }
        .ignore()
    }
}

impl Storage {
    /// Creates a new storage component.
    pub fn new(
        cfg: &WithDir<Config>,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
        network_name: &str,
        finality_threshold_fraction: Ratio<u64>,
        last_emergency_restart: Option<EraId>,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<Self, FatalStorageError> {
        Ok(Storage {
            storage: Arc::new(StorageInner::new(
                cfg,
                hard_reset_to_start_of_era,
                protocol_version,
                network_name,
                finality_threshold_fraction,
                last_emergency_restart,
                verifiable_chunked_hash_activation,
            )?),
            sync_task_limiter: Arc::new(Semaphore::new(cfg.value().max_sync_tasks as usize)),
        })
    }

    pub(crate) fn read_switch_block_header_by_era_id(
        &self,
        switch_block_era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let getter_id = BlockGetterId::SwitchBlock(switch_block_era_id);
        let mut txn = self.storage.env.begin_ro_txn()?;
        Ok(self
            .storage
            .get_block_header(&mut txn, getter_id)?
            .map(|(_block_hash, header)| header))
    }

    pub(crate) fn root_path(&self) -> &Path {
        self.storage.root_path()
    }

    // Note: Methods on `Storage` other than new should disappear and be replaced by an accessor
    // that exposes `StorageInner`, once the latter becomes the publicly shared storage object.

    /// Reads a block from storage.
    #[inline]
    pub fn read_block(&self, block_hash: &BlockHash) -> Result<Option<Block>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_hash(*block_hash);
        let mut txn = self.storage.env.begin_ro_txn()?;
        self.storage.get_block(&mut txn, getter_id)
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    #[inline]
    pub fn read_block_by_height(
        &self,
        block_height: u64,
    ) -> Result<Option<Block>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_height(block_height);
        let mut txn = self.storage.env.begin_ro_txn()?;
        self.storage.get_block(&mut txn, getter_id)
    }

    /// Gets the highest block.
    pub fn read_highest_block(&self) -> Result<Option<Block>, FatalStorageError> {
        let getter_id = BlockGetterId::HighestBlock;
        let mut txn = self.storage.env.begin_ro_txn()?;
        self.storage.get_block(&mut txn, getter_id)
    }

    /// Directly returns a deploy from internal store.
    #[inline]
    pub fn read_deploy_by_hash(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<Deploy>, FatalStorageError> {
        self.storage.get_deploy(&deploy_hash)
    }

    /// Commits a single deploy into storage.
    #[inline]
    pub fn commit_deploy(&self, deploy: &Deploy) -> Result<bool, FatalStorageError> {
        self.storage.commit_deploy(deploy)
    }

    /// Commits a block to storage.
    #[inline]
    pub fn commit_block(&self, block: &Block) -> Result<(), FatalStorageError> {
        self.storage.commit_block(block)
    }
}

#[derive(Eq, PartialEq, Debug)]
struct PopulateIndicesProgress {
    lowest_block_height: u64,
    num_blocks_indexed: u64,
    highest_block_at_startup: u64,
    cursor_start_key: Digest,
}

impl Default for PopulateIndicesProgress {
    fn default() -> Self {
        PopulateIndicesProgress {
            lowest_block_height: u64::MAX,
            num_blocks_indexed: 0,
            highest_block_at_startup: 0,
            cursor_start_key: Digest::default(),
        }
    }
}

impl ToBytes for PopulateIndicesProgress {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.lowest_block_height.write_bytes(writer)?;
        self.num_blocks_indexed.write_bytes(writer)?;
        self.highest_block_at_startup.write_bytes(writer)?;
        self.cursor_start_key.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.lowest_block_height.serialized_length()
            + self.num_blocks_indexed.serialized_length()
            + self.highest_block_at_startup.serialized_length()
            + self.cursor_start_key.serialized_length()
    }
}

impl FromBytes for PopulateIndicesProgress {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (lowest_block_height, remainder) = u64::from_bytes(bytes)?;
        let (num_blocks, remainder) = u64::from_bytes(remainder)?;
        let (highest_block_at_startup, remainder) = u64::from_bytes(remainder)?;
        let (cursor_start_key, remainder) = Digest::from_bytes(remainder)?;
        let value = PopulateIndicesProgress {
            lowest_block_height,
            num_blocks_indexed: num_blocks,
            highest_block_at_startup,
            cursor_start_key,
        };
        Ok((value, remainder))
    }
}

impl StorageInner {
    /// Creates a new inner storage.
    pub fn new(
        cfg: &WithDir<Config>,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
        network_name: &str,
        finality_threshold_fraction: Ratio<u64>,
        last_emergency_restart: Option<EraId>,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<Self, FatalStorageError> {
        let config = cfg.value();

        // Create the database directory.
        let mut root = cfg.with_dir(config.path.clone());
        let network_subdir = root.join(network_name);

        if !network_subdir.exists() {
            fs::create_dir_all(&network_subdir).map_err(|err| {
                FatalStorageError::CreateDatabaseDirectory(network_subdir.clone(), err)
            })?;
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
                    | EnvironmentFlags::NO_TLS
                    // Disable read-ahead. Our data is not stored/read in sequence that would benefit
                    // from the read-ahead.
                    | EnvironmentFlags::NO_READAHEAD,
            )
            // We need at least `max_sync_tasks` readers, add an additional 8 for unforeseen
            // external reads (not likely, but it does not hurt to increase this limit).
            .set_max_readers(config.max_sync_tasks as u32 + 8)
            .set_max_dbs(MAX_DB_COUNT)
            .set_map_size(total_size)
            .open(&root.join(STORAGE_DB_FILENAME))?;

        let block_header_db = env.create_db(Some("block_header"), DatabaseFlags::empty())?;
        let block_body_v1_db = env.create_db(Some("block_body"), DatabaseFlags::empty())?;
        let block_body_v2_db = env.create_db(Some("block_body_merkle"), DatabaseFlags::empty())?;
        let deploy_hashes_db = env.create_db(Some("deploy_hashes"), DatabaseFlags::empty())?;
        let transfer_hashes_db = env.create_db(Some("transfer_hashes"), DatabaseFlags::empty())?;
        let proposer_db = env.create_db(Some("proposers"), DatabaseFlags::empty())?;
        let transfer_db = env.create_db(Some("transfer"), DatabaseFlags::empty())?;
        let block_metadata_db = env.create_db(Some("block_metadata"), DatabaseFlags::empty())?;
        let deploy_db = env.create_db(Some("deploys"), DatabaseFlags::empty())?;
        let deploy_metadata_db = env.create_db(Some("deploy_metadata"), DatabaseFlags::empty())?;
        let finalized_approvals_db =
            env.create_db(Some("finalized_approvals"), DatabaseFlags::empty())?;
        let state_store_db = env.create_db(Some("state_store"), DatabaseFlags::empty())?;
        let indices = IndexDbs::new(&env, state_store_db)?;

        let highest_block_at_startup = {
            let mut txn = env.begin_ro_txn()?;
            indices
                .get_highest_available_block_height(&mut txn)?
                .unwrap_or_default()
        };

        let mut inner = Self {
            root,
            env,
            block_header_db,
            block_body_v1_db,
            block_body_v2_db,
            deploy_hashes_db,
            transfer_hashes_db,
            proposer_db,
            transfer_db,
            block_metadata_db,
            deploy_db,
            deploy_metadata_db,
            finalized_approvals_db,
            state_store_db,
            indices,
            highest_block_at_startup,
            enable_mem_deduplication: config.enable_mem_deduplication,
            serialized_item_pool: RwLock::new(ObjectPool::new(config.mem_pool_prune_interval)),
            finality_threshold_fraction,
            last_emergency_restart,
            verifiable_chunked_hash_activation,
        };

        inner.populate_indices()?;

        if let Some(era_to_reset_to) = hard_reset_to_start_of_era {
            inner.hard_reset(era_to_reset_to, protocol_version)?;
        }

        Ok(inner)
    }

    /// If required, populates the `indices` databases by iterating the block header database.
    ///
    /// This is essentially a one-time migration from using in-mem indices to on-disk indices.  When
    /// it has been completed once, all further calls to `populate_indices` will be no-ops.
    ///
    /// The ongoing progress is committed every 10,000th block, providing reasonable ability for
    /// resumption in the case of cancellation.
    fn populate_indices(&mut self) -> Result<(), FatalStorageError> {
        let mut ro_txn = self.env.begin_ro_txn()?;
        if self
            .indices
            .get_highest_available_block_height(&mut ro_txn)?
            .is_some()
        {
            info!("indexing block store completed previously");
            // We already completed indexing the database previously.
            return Ok(());
        }

        let mut progress = ro_txn
            .get_value_bytesrepr::<_, PopulateIndicesProgress>(self.state_store_db, &PROGRESS_KEY)?
            .unwrap_or_default();

        let mut rw_txn = self.env.begin_rw_txn()?;
        let mut block_header_cursor = ro_txn.open_ro_cursor(self.block_header_db)?;
        #[allow(clippy::needless_late_init)]
        let mut iter;
        if progress.cursor_start_key == Digest::default() {
            iter = block_header_cursor.iter();
        } else {
            iter = block_header_cursor.iter_from(progress.cursor_start_key);
        }
        loop {
            info!(
                "indexing block store: completed {} blocks",
                progress.num_blocks_indexed
            );
            for (raw_key, raw_val) in &mut iter {
                let block_hash = BlockHash::new(
                    Digest::try_from(raw_key)
                        .map_err(|error| LmdbExtError::DataCorrupted(Box::new(error)))?,
                );
                let block_header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
                let block_height = block_header.height();
                let era_id = block_header.era_id();
                progress.lowest_block_height = cmp::min(progress.lowest_block_height, block_height);
                progress.highest_block_at_startup =
                    cmp::max(progress.highest_block_at_startup, block_height);

                self.indices.put_to_block_height_index(
                    &mut rw_txn,
                    block_height,
                    block_hash,
                    era_id,
                    block_header.protocol_version(),
                )?;

                if block_header.is_switch_block() {
                    self.indices.put_to_switch_block_era_id_index(
                        &mut rw_txn,
                        era_id,
                        &block_hash,
                    )?;
                }

                if let Some(block_body) =
                    self.get_body_for_block_header(&mut rw_txn, &block_header)?
                {
                    if self.is_v1_block(&block_header) {
                        self.indices.put_to_v1_body_hash_index(
                            &mut rw_txn,
                            block_header.body_hash(),
                            block_hash,
                            block_height,
                        )?;
                    }

                    for deploy_hash in block_body
                        .deploy_hashes()
                        .iter()
                        .chain(block_body.transfer_hashes().iter())
                    {
                        self.indices.put_to_deploy_hash_index(
                            &mut rw_txn,
                            deploy_hash,
                            block_hash,
                            block_height,
                        )?;
                    }
                }

                progress.num_blocks_indexed += 1;
                if progress.num_blocks_indexed % 10000 == 0 {
                    break;
                }
            }
            match iter.next() {
                Some((raw_key, _)) => {
                    progress.cursor_start_key = Digest::try_from(raw_key)
                        .map_err(|error| LmdbExtError::DataCorrupted(Box::new(error)))?
                }
                None => break,
            };
            block_header_cursor = ro_txn.open_ro_cursor(self.block_header_db)?;
            iter = block_header_cursor.iter_from(progress.cursor_start_key);
            rw_txn.put_value_bytesrepr(self.state_store_db, &PROGRESS_KEY, &progress, true)?;
            rw_txn.commit()?;
            rw_txn = self.env.begin_rw_txn()?;
        }

        let lowest_block_height = if progress.num_blocks_indexed == 0 {
            0
        } else {
            progress.lowest_block_height
        };
        self.indices
            .put_lowest_available_block_height(&mut rw_txn, lowest_block_height)?;
        self.indices
            .put_highest_available_block_height(&mut rw_txn, progress.highest_block_at_startup)?;

        rw_txn.commit()?;
        self.highest_block_at_startup = progress.highest_block_at_startup;
        info!(
            "indexing block store complete - indexed {} blocks",
            progress.num_blocks_indexed
        );

        Ok(())
    }

    fn hard_reset(
        &mut self,
        era_to_reset_to: EraId,
        current_protocol_version: ProtocolVersion,
    ) -> Result<(), FatalStorageError> {
        info!(%era_to_reset_to, "hard resetting storage");
        let mut txn = self.env.begin_rw_txn()?;

        // Clear all DBs if we're resetting to era 0.
        if era_to_reset_to.value() == 0 {
            // To avoid removing valid blocks on a restart outside of an upgrade shutdown, only
            // remove blocks for an older protocol version.
            if let Some((_block_hash, block_header)) =
                self.get_block_header(&mut txn, BlockGetterId::with_block_height(0))?
            {
                if block_header.protocol_version() >= current_protocol_version {
                    return Ok(());
                }
            }
            info!("hard-resetting storage to genesis");
            txn.clear_db(self.block_header_db)?;
            txn.clear_db(self.block_body_v1_db)?;
            txn.clear_db(self.block_body_v2_db)?;
            txn.clear_db(self.deploy_hashes_db)?;
            txn.clear_db(self.transfer_hashes_db)?;
            txn.clear_db(self.proposer_db)?;
            txn.clear_db(self.transfer_db)?;
            txn.clear_db(self.block_metadata_db)?;
            // Don't clear the deploy DB.
            txn.clear_db(self.deploy_metadata_db)?;
            txn.clear_db(self.finalized_approvals_db)?;
            self.indices.clear_all(&mut txn)?;
            self.highest_block_at_startup = 0;
            txn.commit()?;
            return Ok(());
        }

        // Get the height of the highest block to not delete, i.e. the switch block of
        // `era_to_reset_to - 1`.
        let mut block_height = match self
            .get_block_header(&mut txn, BlockGetterId::SwitchBlock(era_to_reset_to - 1))?
        {
            Some(hash_and_header) => hash_and_header.1.height(),
            None => {
                // If we don't have the switch block, there's nothing to purge.
                return Ok(());
            }
        };

        // Set this as the new highest block, and the new lowest if required.
        self.indices
            .put_highest_available_block_height(&mut txn, block_height)?;
        if block_height
            < self
                .indices
                .get_lowest_available_block_height(&mut txn)?
                .unwrap_or_default()
        {
            self.indices
                .put_lowest_available_block_height(&mut txn, block_height)?;
        }
        self.highest_block_at_startup = block_height;

        // Iterate all higher blocks, deleting relevant data.
        loop {
            block_height += 1;

            // Gather the data needed to remove entries before we start actually deleting them.
            let (block_hash, header) = match self
                .get_block_header(&mut txn, BlockGetterId::with_block_height(block_height))?
            {
                Some(hash_and_header) => hash_and_header,
                None => break,
            };
            let maybe_block_body = self.get_body_for_block_header(&mut txn, &header)?;

            // Delete the relevant entries.
            txn.delete(self.block_header_db, &block_hash)?;
            txn.delete(self.transfer_db, &block_hash)?;
            txn.delete(self.block_metadata_db, &block_hash)?;
            self.indices
                .delete_from_block_height_index(&mut txn, header.height())?;
            if header.is_switch_block() {
                self.indices
                    .delete_from_switch_block_era_id_index(&mut txn, header.era_id())?;
            }
            if self.is_v1_block(&header) {
                let should_delete_v1_block_body = self.indices.delete_from_v1_body_hash_index(
                    &mut txn,
                    header.body_hash(),
                    block_hash,
                    header.height(),
                )?;
                if should_delete_v1_block_body {
                    txn.delete(self.block_body_v1_db, header.body_hash())?;
                }
            }

            let block_body = match maybe_block_body {
                Some(body) => body,
                None => continue,
            };

            for deploy_hash in block_body
                .deploy_hashes()
                .iter()
                .chain(block_body.transfer_hashes().iter())
            {
                // Don't delete the actual deploy - just the metadata and finalized approvals.
                txn.delete(self.deploy_metadata_db, deploy_hash)?;
                txn.delete(self.finalized_approvals_db, deploy_hash)?;
                self.indices
                    .delete_from_deploy_hash_index(&mut txn, deploy_hash)?;
            }
        }

        txn.commit()?;
        Ok(())
    }

    /// Handles a state store request.
    fn handle_state_store_request<REv>(
        &self,
        _effect_builder: EffectBuilder<REv>,
        req: StateStoreRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
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
            StateStoreRequest::Load { key, responder } => {
                let bytes = self.get_from_state_store(&key)?;
                Ok(responder.respond(bytes).ignore())
            }
        }
    }

    /// Reads from the state storage DB.
    /// If key is non-empty, returns bytes from under the key. Otherwise returns `Ok(None)`.
    /// May also fail with storage errors.
    fn get_from_state_store<K: AsRef<[u8]>>(
        &self,
        key: &K,
    ) -> Result<Option<Vec<u8>>, FatalStorageError> {
        let txn = self.env.begin_ro_txn()?;
        let bytes = match txn.get(self.state_store_db, &key) {
            Ok(slice) => Some(slice.to_owned()),
            Err(lmdb::Error::NotFound) => None,
            Err(err) => return Err(err.into()),
        };
        Ok(bytes)
    }

    /// Returns the path to the storage folder.
    fn root_path(&self) -> &Path {
        &self.root
    }

    fn handle_net_request_incoming<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        incoming: &NetRequestIncoming,
    ) -> Result<Effects<Event>, GetRequestError>
    where
        REv: From<NetworkRequest<Message>> + Send,
    {
        if self.enable_mem_deduplication {
            let unique_id = incoming.message.unique_id();

            let serialized_item_pool = self
                .serialized_item_pool
                .read()
                .map_err(FatalStorageError::from)?;
            if let Some(serialized_item) =
                serialized_item_pool.get(AsRef::<[u8]>::as_ref(&unique_id))
            {
                // We found an item in the pool. We can short-circuit all deserialization/
                // serialization and return the canned item immediately.
                let found = Message::new_get_response_from_serialized(
                    incoming.message.tag(),
                    serialized_item,
                );
                return Ok(effect_builder.send_message(incoming.sender, found).ignore());
            }
        }

        let mut txn = self.env.begin_ro_txn().map_err(FatalStorageError::from)?;
        match incoming.message {
            NetRequest::Deploy(ref serialized_id) => {
                let deploy_hash = decode_item_id::<Deploy>(serialized_id)?;
                let opt_item: Option<Deploy> = txn
                    .get_value(self.deploy_db, &deploy_hash)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    deploy_hash,
                    opt_item,
                )?)
            }
            NetRequest::FinalizedApprovals(ref serialized_id) => {
                let deploy_hash = decode_item_id::<FinalizedApprovalsWithId>(serialized_id)?;
                let opt_item = self
                    .get_deploy_with_finalized_approvals(&mut txn, &deploy_hash)
                    .map_err(FatalStorageError::from)?
                    .map(|deploy| {
                        FinalizedApprovalsWithId::new(
                            deploy_hash,
                            FinalizedApprovals::new(deploy.into_naive().approvals().clone()),
                        )
                    });

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    deploy_hash,
                    opt_item,
                )?)
            }
            NetRequest::Block(ref serialized_id) => {
                let block_hash = decode_item_id::<Block>(serialized_id)?;
                let getter_id = BlockGetterId::with_block_hash(block_hash);
                let opt_item = self
                    .get_block(&mut txn, getter_id)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    block_hash,
                    opt_item,
                )?)
            }
            NetRequest::GossipedAddress(_) => Err(GetRequestError::GossipedAddressNotGettable),
            NetRequest::BlockAndMetadataByHeight(ref serialized_id) => {
                let block_height = decode_item_id::<BlockWithMetadata>(serialized_id)?;
                let getter_id = BlockGetterId::with_block_height(block_height);
                let opt_item = self
                    .get_block_with_sigs(&mut txn, getter_id)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    block_height,
                    opt_item,
                )?)
            }
            NetRequest::BlockHeaderByHash(ref serialized_id) => {
                let block_hash = decode_item_id::<BlockHeader>(serialized_id)?;
                let getter_id = BlockGetterId::with_block_hash(block_hash);
                let opt_item = self
                    .get_block_header(&mut txn, getter_id)
                    .map_err(FatalStorageError::from)?
                    .map(|(_block_hash, header)| header);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    block_hash,
                    opt_item,
                )?)
            }
            NetRequest::BlockHeaderAndFinalitySignaturesByHeight(ref serialized_id) => {
                let block_height = decode_item_id::<BlockHeaderWithMetadata>(serialized_id)?;
                let getter_id = BlockGetterId::with_block_height(block_height);
                let opt_item = self
                    .get_block_header_with_enough_sigs(&mut txn, getter_id)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    block_height,
                    opt_item,
                )?)
            }
            NetRequest::BlockAndDeploys(ref serialized_id) => {
                let block_hash = decode_item_id::<BlockAndDeploys>(serialized_id)?;
                let opt_item = self
                    .get_block_and_deploys(&mut txn, block_hash)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    block_hash,
                    opt_item,
                )?)
            }
        }
    }

    /// Handles a storage request.
    fn handle_storage_request<REv>(
        &self,
        request: StorageRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
        let getter_id = BlockGetterId::from(&request);
        Ok(match request {
            StorageRequest::PutBlock { block, responder } => {
                responder.respond(self.commit_block(&*block)?).ignore()
            }
            StorageRequest::GetBlock { responder, .. }
            | StorageRequest::GetHighestBlock { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_block = self.get_block(&mut txn, getter_id)?;
                responder.respond(maybe_block).ignore()
            }
            StorageRequest::GetBlockHeader { responder, .. }
            | StorageRequest::GetHighestBlockHeader { responder }
            | StorageRequest::GetSwitchBlockHeaderAtEraId { responder, .. }
            | StorageRequest::GetBlockHeaderForDeploy { responder, .. }
            | StorageRequest::GetBlockHeaderByHeight { responder, .. } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_block_header = self
                    .get_block_header(&mut txn, getter_id)?
                    .map(|(_block_hash, header)| header);
                responder.respond(maybe_block_header).ignore()
            }
            StorageRequest::GetBlockHeaderAndSufficientFinalitySignaturesByHeight {
                responder,
                ..
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_block_header =
                    self.get_block_header_with_enough_sigs(&mut txn, getter_id)?;
                responder.respond(maybe_block_header).ignore()
            }

            StorageRequest::GetBlockAndMetadataByHash { responder, .. }
            | StorageRequest::GetBlockAndMetadataByHeight { responder, .. }
            | StorageRequest::GetHighestBlockWithMetadata { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_block_and_metadata = self.get_block_with_sigs(&mut txn, getter_id)?;
                responder.respond(maybe_block_and_metadata).ignore()
            }
            StorageRequest::GetBlockAndSufficientFinalitySignaturesByHeight {
                responder, ..
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_block_and_metadata =
                    self.get_block_with_enough_sigs(&mut txn, getter_id)?;
                responder.respond(maybe_block_and_metadata).ignore()
            }
            StorageRequest::CheckBlockHeaderExistence {
                block_height,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let exists = self
                    .indices
                    .exists_in_block_height_index(&mut txn, block_height)?;
                responder.respond(exists).ignore()
            }
            StorageRequest::GetBlockTransfers {
                block_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_transfers = self.get_transfers(&mut txn, &block_hash)?;
                responder.respond(maybe_transfers).ignore()
            }
            StorageRequest::PutDeploy { deploy, responder } => {
                responder.respond(self.commit_deploy(&*deploy)?).ignore()
            }
            StorageRequest::GetDeploys {
                deploy_hashes,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let deploys =
                    self.get_deploys_with_finalized_approvals(&mut txn, deploy_hashes.as_slice())?;
                responder.respond(deploys).ignore()
            }
            StorageRequest::PutExecutionResults {
                block_hash,
                execution_results,
                responder,
            } => responder
                .respond(self.put_execution_results(*block_hash, execution_results)?)
                .ignore(),
            StorageRequest::GetDeployAndMetadata {
                deploy_hash,
                responder,
            } => {
                let deploy_and_metadata = self.get_deploy_and_metadata(deploy_hash)?;
                responder.respond(deploy_and_metadata).ignore()
            }
            StorageRequest::PutBlockSignatures {
                signatures,
                responder,
            } => {
                let outcome = self.commit_block_signatures(signatures)?;
                responder.respond(outcome).ignore()
            }
            StorageRequest::GetBlockSignatures {
                block_hash,
                responder,
            } => {
                let result =
                    self.get_block_signatures(&mut self.env.begin_ro_txn()?, &block_hash)?;
                responder.respond(result).ignore()
            }
            StorageRequest::GetFinalizedBlocks { ttl, responder } => {
                responder.respond(self.get_finalized_blocks(ttl)?).ignore()
            }
            StorageRequest::PutBlockHeader {
                block_header,
                responder,
            } => responder
                .respond(self.commit_block_header(&*block_header)?)
                .ignore(),
            StorageRequest::UpdateLowestAvailableBlockHeight { height, responder } => {
                self.update_lowest_available_block_height(height)?;
                responder.respond(()).ignore()
            }
            StorageRequest::GetAvailableBlockRange { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let available_range = self.get_available_block_range(&mut txn)?;
                responder.respond(available_range).ignore()
            }
            StorageRequest::StoreFinalizedApprovals {
                ref deploy_hash,
                ref finalized_approvals,
                responder,
            } => responder
                .respond(self.store_finalized_approvals(deploy_hash, finalized_approvals)?)
                .ignore(),
            StorageRequest::PutBlockAndDeploys { block, responder } => responder
                .respond(self.commit_block_and_deploys(&*block)?)
                .ignore(),
            StorageRequest::GetBlockAndDeploys {
                block_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_block_and_deploys = self.get_block_and_deploys(&mut txn, block_hash)?;
                responder.respond(maybe_block_and_deploys).ignore()
            }
        })
    }

    /// Put a single deploy into storage.
    fn commit_deploy(&self, deploy: &Deploy) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let outcome = self.put_deploy(&mut txn, deploy, None)?;
        txn.commit()?;
        Ok(outcome)
    }

    fn put_deploy(
        &self,
        txn: &mut RwTransaction,
        deploy: &Deploy,
        maybe_block_hash_and_height: Option<(BlockHash, u64)>,
    ) -> Result<bool, FatalStorageError> {
        let outcome = txn.put_value(self.deploy_db, deploy.id(), deploy, false)?;
        if let Some((block_hash, block_height)) = maybe_block_hash_and_height {
            self.indices
                .put_to_deploy_hash_index(txn, deploy.id(), block_hash, block_height)?;
        }
        Ok(outcome)
    }

    /// Puts block and its deploys into storage.
    ///
    /// Returns `Ok` only if the block and all deploys were successfully written.
    fn commit_block_and_deploys(
        &self,
        block_and_deploys: &BlockAndDeploys,
    ) -> Result<(), FatalStorageError> {
        let BlockAndDeploys { block, deploys } = block_and_deploys;

        let mut txn = self.env.begin_rw_txn()?;
        self.put_block(&mut txn, block)?;

        let maybe_block_hash_and_height = Some((*block.hash(), block.height()));
        for deploy in deploys {
            self.put_deploy(&mut txn, deploy, maybe_block_hash_and_height)?;
        }

        txn.commit()?;
        Ok(())
    }

    fn commit_block_signatures(
        &self,
        signatures: BlockSignatures,
    ) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let old_data: Option<BlockSignatures> =
            txn.get_value(self.block_metadata_db, &signatures.block_hash)?;
        let new_data = match old_data {
            None => signatures,
            Some(mut data) => {
                for (public_key, sig) in signatures.proofs {
                    data.insert_proof(public_key, sig);
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
        Ok(outcome)
    }

    fn get_block_header<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        getter_id: BlockGetterId,
    ) -> Result<Option<(BlockHash, BlockHeader)>, FatalStorageError> {
        match getter_id {
            BlockGetterId::BlockHash {
                block_hash,
                only_from_available_range,
            } => {
                let header: BlockHeader = match txn.get_value(self.block_header_db, &block_hash)? {
                    Some(header) => header,
                    None => return Ok(None),
                };
                if only_from_available_range {
                    let available_range = self.get_available_block_range(txn)?;
                    if !available_range.contains(header.height()) {
                        return Ok(None);
                    }
                }
                Ok(Some((block_hash, header)))
            }
            BlockGetterId::BlockHeight {
                block_height,
                only_from_available_range,
            } => {
                if only_from_available_range {
                    let available_range = self.get_available_block_range(txn)?;
                    if !available_range.contains(block_height) {
                        return Ok(None);
                    }
                }
                match self
                    .indices
                    .get_from_block_height_index(txn, block_height)?
                {
                    Some(value) => Ok(txn
                        .get_value(self.block_header_db, &value.block_hash)?
                        .map(|header| (value.block_hash, header))),
                    None => Ok(None),
                }
            }
            BlockGetterId::HighestBlock => {
                match self.indices.get_highest_available_block_info(txn)? {
                    Some(info) => Ok(txn
                        .get_value(self.block_header_db, &info.block_hash)?
                        .map(|header| (info.block_hash, header))),
                    None => Ok(None),
                }
            }
            BlockGetterId::SwitchBlock(era_id) => {
                match self
                    .indices
                    .get_from_switch_block_era_id_index(txn, era_id)?
                {
                    Some(block_hash) => Ok(txn
                        .get_value(self.block_header_db, &block_hash)?
                        .map(|header| (block_hash, header))),
                    None => Ok(None),
                }
            }
            BlockGetterId::DeployHash(deploy_hash) => {
                match self.indices.get_from_deploy_hash_index(txn, &deploy_hash)? {
                    Some(block_hash_and_height) => Ok(txn
                        .get_value(self.block_header_db, &block_hash_and_height.block_hash)?
                        .map(|header| (block_hash_and_height.block_hash, header))),
                    None => Ok(None),
                }
            }
            BlockGetterId::None => {
                error!("cannot get a block header with getter id none");
                Err(FatalStorageError::InvalidBlockGetterId)
            }
        }
    }

    fn get_block_header_with_sigs<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        getter_id: BlockGetterId,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let (block_hash, block_header) = match self.get_block_header(txn, getter_id)? {
            Some(hash_and_header) => hash_and_header,
            None => return Ok(None),
        };
        let block_signatures: BlockSignatures = txn
            .get_value(self.block_metadata_db, &block_hash)?
            .unwrap_or_else(|| BlockSignatures::new(block_hash, block_header.era_id()));
        Ok(Some(BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        }))
    }

    /// Retrieves block header and block signatures; returns `None` if there are less than the fault
    /// tolerance threshold signatures or if the block is from before the most recent emergency
    /// upgrade, or if the block is from era 0.
    fn get_block_header_with_enough_sigs<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        getter_id: BlockGetterId,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        } = match self.get_block_header_with_sigs(txn, getter_id)? {
            Some(header_with_metadata) => header_with_metadata,
            None => return Ok(None),
        };

        if let Some(last_emergency_restart) = self.last_emergency_restart {
            if block_header.era_id() <= last_emergency_restart {
                debug!(
                    ?block_header,
                    ?last_emergency_restart,
                    "block signatures from before last emergency restart requested"
                );
                return Ok(None);
            }
        }

        if block_header.era_id().value() == 0 {
            debug!(?block_header, "block signatures from era 0 requested");
            return Ok(None);
        }

        let getter_id = BlockGetterId::SwitchBlock(block_header.era_id() - 1);
        let switch_block_header = match self.get_block_header(txn, getter_id)? {
            Some((_block_hash, header)) => header,
            None => return Ok(None),
        };

        let validator_weights = match switch_block_header.next_era_validator_weights() {
            Some(validator_weights) => validator_weights,
            None => {
                return Err(FatalStorageError::InvalidSwitchBlock(Box::new(
                    switch_block_header,
                )))
            }
        };

        let block_signatures = match consensus::get_minimal_set_of_signatures(
            validator_weights,
            self.finality_threshold_fraction,
            block_signatures,
        ) {
            Some(sigs) => sigs,
            None => return Ok(None),
        };

        Ok(Some(BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        }))
    }

    fn get_block<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        getter_id: BlockGetterId,
    ) -> Result<Option<Block>, FatalStorageError> {
        let (block_hash, header) = match self.get_block_header(txn, getter_id)? {
            Some(hash_and_header) => hash_and_header,
            None => return Ok(None),
        };
        match self.get_body_for_block_header(txn, &header)? {
            Some(body) => Ok(Some(Block::new_unchecked(block_hash, header, body))),
            None => Ok(None),
        }
    }

    fn get_block_with_sigs<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        getter_id: BlockGetterId,
    ) -> Result<Option<BlockWithMetadata>, FatalStorageError> {
        let BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        } = match self.get_block_header_with_sigs(txn, getter_id)? {
            Some(header_with_metadata) => header_with_metadata,
            None => return Ok(None),
        };
        let block_body = match self.get_body_for_block_header(txn, &block_header)? {
            Some(block_body) => block_body,
            None => return Ok(None),
        };
        let block = Block::new_unchecked(block_signatures.block_hash, block_header, block_body);
        Ok(Some(BlockWithMetadata {
            block,
            finality_signatures: block_signatures,
        }))
    }

    /// Retrieves block and block signatures; returns `None` if there are less than the fault
    /// tolerance threshold signatures or if the block is from before the most recent emergency
    /// upgrade, or if the block is from era 0.
    fn get_block_with_enough_sigs<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        getter_id: BlockGetterId,
    ) -> Result<Option<BlockWithMetadata>, FatalStorageError> {
        let BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        } = match self.get_block_header_with_enough_sigs(txn, getter_id)? {
            Some(header_with_metadata) => header_with_metadata,
            None => return Ok(None),
        };
        let block_body = match self.get_body_for_block_header(txn, &block_header)? {
            Some(block_body) => block_body,
            None => return Ok(None),
        };
        let block = Block::new_unchecked(block_signatures.block_hash, block_header, block_body);
        Ok(Some(BlockWithMetadata {
            block,
            finality_signatures: block_signatures,
        }))
    }

    fn get_body_for_block_header<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_header: &BlockHeader,
    ) -> Result<Option<BlockBody>, LmdbExtError> {
        if self.is_v1_block(block_header) {
            txn.get_value(self.block_body_v1_db, block_header.body_hash())
        } else {
            let deploy_hashes_with_proof: MerkleLinkedListNode<Vec<DeployHash>> = match self
                .get_merkle_linked_list_node(txn, self.deploy_hashes_db, block_header.body_hash())?
            {
                Some(deploy_hashes_with_proof) => deploy_hashes_with_proof,
                None => return Ok(None),
            };
            let transfer_hashes_with_proof: MerkleLinkedListNode<Vec<DeployHash>> = match self
                .get_merkle_linked_list_node(
                    txn,
                    self.transfer_hashes_db,
                    deploy_hashes_with_proof.merkle_proof_of_rest(),
                )? {
                Some(transfer_hashes_with_proof) => transfer_hashes_with_proof,
                None => return Ok(None),
            };
            let proposer_with_proof: MerkleLinkedListNode<PublicKey> = match self
                .get_merkle_linked_list_node(
                    txn,
                    self.proposer_db,
                    transfer_hashes_with_proof.merkle_proof_of_rest(),
                )? {
                Some(proposer_with_proof) => {
                    debug_assert_eq!(
                        *proposer_with_proof.merkle_proof_of_rest(),
                        Digest::SENTINEL_RFOLD
                    );
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
    }

    fn get_merkle_linked_list_node<Tx: Transaction, T: FromBytes>(
        &self,
        txn: &mut Tx,
        part_database: Database,
        key_to_block_body_db: &Digest,
    ) -> Result<Option<MerkleLinkedListNode<T>>, LmdbExtError> {
        let (part_to_value_db, merkle_proof_of_rest): (Digest, Digest) =
            match txn.get_value_bytesrepr(self.block_body_v2_db, key_to_block_body_db)? {
                Some(slice) => slice,
                None => return Ok(None),
            };
        let value = match txn.get_value_bytesrepr(part_database, &part_to_value_db)? {
            Some(value) => value,
            None => return Ok(None),
        };
        Ok(Some(MerkleLinkedListNode::new(value, merkle_proof_of_rest)))
    }

    /// Writes a block to storage, updating indices as necessary.
    fn commit_block(&self, block: &Block) -> Result<(), FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        self.put_block(&mut txn, block)?;
        txn.commit()?;
        Ok(())
    }

    fn put_block(&self, txn: &mut RwTransaction, block: &Block) -> Result<(), FatalStorageError> {
        if self.is_v1_block(block.header()) {
            txn.put_value(
                self.block_body_v1_db,
                block.header().body_hash(),
                block.body(),
                true,
            )?;
            self.indices.put_to_v1_body_hash_index(
                txn,
                block.header().body_hash(),
                *block.hash(),
                block.height(),
            )?;
        } else {
            self.put_single_block_body_v2(txn, block.body())?;
        };

        self.put_block_header(txn, block.hash(), block.header())?;

        let current_highest = self
            .indices
            .get_highest_available_block_height(txn)?
            .unwrap_or_default();
        if block.height() > current_highest {
            self.indices
                .put_highest_available_block_height(txn, block.height())?;
        }

        for deploy_hash in block
            .body()
            .deploy_hashes()
            .iter()
            .chain(block.body().transfer_hashes().iter())
        {
            self.indices.put_to_deploy_hash_index(
                txn,
                deploy_hash,
                *block.hash(),
                block.height(),
            )?;
        }
        Ok(())
    }

    fn commit_block_header(&self, block_header: &BlockHeader) -> Result<(), FatalStorageError> {
        let block_hash = block_header.hash(self.verifiable_chunked_hash_activation);
        let mut txn = self.env.begin_rw_txn()?;
        self.put_block_header(&mut txn, &block_hash, block_header)?;
        txn.commit()?;
        Ok(())
    }

    fn put_block_header(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
        block_header: &BlockHeader,
    ) -> Result<(), FatalStorageError> {
        txn.put_value(self.block_header_db, &block_hash, block_header, true)?;
        if block_header.is_switch_block() {
            self.indices.put_to_switch_block_era_id_index(
                txn,
                block_header.era_id(),
                block_hash,
            )?;
        }
        self.indices.put_to_block_height_index(
            txn,
            block_header.height(),
            *block_hash,
            block_header.era_id(),
            block_header.protocol_version(),
        )?;
        Ok(())
    }

    fn put_merkle_block_body_part<'a, T>(
        &self,
        txn: &mut RwTransaction,
        part_database: Database,
        merklized_block_body_part: &MerkleBlockBodyPart<'a, T>,
    ) -> Result<bool, LmdbExtError>
    where
        T: ToBytes,
    {
        // It's possible the value is already present (ie, if it is a block proposer).
        // We put the value and rest hashes in first, since we need that present even if the value
        // is already there.
        if !txn.put_value_bytesrepr(
            self.block_body_v2_db,
            merklized_block_body_part.merkle_linked_list_node_hash(),
            &merklized_block_body_part.value_and_rest_hashes_pair(),
            true,
        )? {
            return Ok(false);
        };

        if !txn.put_value_bytesrepr(
            part_database,
            merklized_block_body_part.value_hash(),
            merklized_block_body_part.value(),
            true,
        )? {
            return Ok(false);
        };
        Ok(true)
    }

    /// Writes a single Merklized block body in a separate transaction to storage.
    fn put_single_block_body_v2(
        &self,
        txn: &mut RwTransaction,
        block_body: &BlockBody,
    ) -> Result<bool, LmdbExtError> {
        let merkle_block_body = block_body.merklize();
        let MerkleBlockBody {
            deploy_hashes,
            transfer_hashes,
            proposer,
        } = &merkle_block_body;
        if !self.put_merkle_block_body_part(txn, self.deploy_hashes_db, deploy_hashes)? {
            return Ok(false);
        };
        if !self.put_merkle_block_body_part(txn, self.transfer_hashes_db, transfer_hashes)? {
            return Ok(false);
        };
        if !self.put_merkle_block_body_part(txn, self.proposer_db, proposer)? {
            return Ok(false);
        };
        Ok(true)
    }

    fn put_execution_results(
        &self,
        block_hash: BlockHash,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) -> Result<(), FatalStorageError> {
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

            // Update metadata and write back to db.
            metadata
                .execution_results
                .insert(block_hash, execution_result);
            txn.put_value(self.deploy_metadata_db, &deploy_hash, &metadata, true)?;
        }

        txn.put_value(self.transfer_db, &block_hash, &transfers, true)?;
        txn.commit()?;
        Ok(())
    }

    fn get_deploy_and_metadata(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<(DeployWithFinalizedApprovals, DeployMetadataExt)>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;

        let deploy = match self.get_deploy_with_finalized_approvals(&mut txn, &deploy_hash)? {
            Some(deploy) => deploy,
            None => return Ok(None),
        };

        // Missing metadata is filled using a default.
        let metadata_ext = match self.get_deploy_metadata(&mut txn, &deploy_hash)? {
            Some(metadata) => DeployMetadataExt::from(metadata),
            None => match self
                .indices
                .get_from_deploy_hash_index(&mut txn, &deploy_hash)?
            {
                Some(block_hash_and_height) => DeployMetadataExt::from(block_hash_and_height),
                None => DeployMetadataExt::Empty,
            },
        };

        Ok(Some((deploy, metadata_ext)))
    }

    /// Retrieves single block and all of its deploys.
    ///
    /// If any of the deploys can't be found, returns `Ok(None)`.
    fn get_block_and_deploys<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: BlockHash,
    ) -> Result<Option<BlockAndDeploys>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_hash(block_hash);
        let block = match self.get_block(txn, getter_id)? {
            Some(block) => block,
            None => return Ok(None),
        };

        let deploys_count = block.deploy_hashes().len() + block.transfer_hashes().len();
        let mut deploys: Vec<Deploy> = Vec::with_capacity(deploys_count);

        for deploy_hash in block
            .deploy_hashes()
            .iter()
            .chain(block.transfer_hashes().iter())
        {
            match txn.get_value(self.deploy_db, deploy_hash)? {
                Some(deploy) => deploys.push(deploy),
                None => return Ok(None),
            }
        }

        Ok(Some(BlockAndDeploys { block, deploys }))
    }

    /// Returns vector blocks that satisfy the predicate, starting from the latest one and following
    /// the ancestry chain.
    fn get_blocks_while<F, Tx: Transaction>(
        &self,
        txn: &mut Tx,
        predicate: F,
    ) -> Result<Vec<Block>, FatalStorageError>
    where
        F: Fn(&Block) -> bool,
    {
        let mut next_block = self.get_block(txn, BlockGetterId::HighestBlock)?;
        let mut blocks = Vec::new();
        loop {
            match next_block {
                None => break,
                Some(block) if !predicate(&block) => break,
                Some(block) => {
                    next_block = match block.parent() {
                        None => None,
                        Some(parent_hash) => {
                            self.get_block(txn, BlockGetterId::with_block_hash(*parent_hash))?
                        }
                    };
                    blocks.push(block);
                }
            }
        }
        Ok(blocks)
    }

    /// Returns the vector of blocks that could still have deploys whose TTL hasn't expired yet.
    fn get_finalized_blocks(&self, ttl: TimeDiff) -> Result<Vec<Block>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        // We're interested in deploys whose TTL hasn't expired yet.
        let ttl_not_expired = |block: &Block| block.timestamp().elapsed() < ttl;
        self.get_blocks_while(&mut txn, ttl_not_expired)
    }

    fn get_deploy(&self, deploy_hash: &DeployHash) -> Result<Option<Deploy>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        Ok(txn.get_value(self.deploy_db, deploy_hash)?)
    }

    /// Retrieves a set of deploys from storage, along with their potential finalized approvals.
    fn get_deploys_with_finalized_approvals<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        deploy_hashes: &[DeployHash],
    ) -> Result<SmallVec<[Option<DeployWithFinalizedApprovals>; 1]>, LmdbExtError> {
        deploy_hashes
            .iter()
            .map(|deploy_hash| self.get_deploy_with_finalized_approvals(txn, deploy_hash))
            .collect()
    }

    /// Retrieves a single deploy along with its finalized approvals from storage
    fn get_deploy_with_finalized_approvals<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        deploy_hash: &DeployHash,
    ) -> Result<Option<DeployWithFinalizedApprovals>, LmdbExtError> {
        let maybe_original_deploy = txn.get_value(self.deploy_db, deploy_hash)?;
        if let Some(deploy) = maybe_original_deploy {
            let maybe_finalized_approvals =
                txn.get_value(self.finalized_approvals_db, deploy_hash)?;
            Ok(Some(DeployWithFinalizedApprovals::new(
                deploy,
                maybe_finalized_approvals,
            )))
        } else {
            Ok(None)
        }
    }

    /// Retrieves deploy metadata associated with deploy.
    ///
    /// If no deploy metadata is stored for the specific deploy, an empty metadata instance will be
    /// created, but not stored.
    fn get_deploy_metadata<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        deploy_hash: &DeployHash,
    ) -> Result<Option<DeployMetadata>, FatalStorageError> {
        Ok(txn.get_value(self.deploy_metadata_db, deploy_hash)?)
    }

    /// Retrieves transfers associated with block.
    ///
    /// If no transfers are stored for the block, an empty transfers instance will be
    /// created, but not stored.
    fn get_transfers<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, FatalStorageError> {
        Ok(txn.get_value(self.transfer_db, block_hash)?)
    }

    /// Retrieves finality signatures for a block with a given block hash
    fn get_block_signatures<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, FatalStorageError> {
        Ok(txn.get_value(self.block_metadata_db, block_hash)?)
    }

    /// Stores a set of finalized approvals.
    fn store_finalized_approvals(
        &self,
        deploy_hash: &DeployHash,
        finalized_approvals: &FinalizedApprovals,
    ) -> Result<(), FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let maybe_original_deploy: Option<Deploy> = txn.get_value(self.deploy_db, &deploy_hash)?;
        let original_deploy =
            maybe_original_deploy.ok_or(FatalStorageError::UnexpectedFinalizedApprovals {
                deploy_hash: *deploy_hash,
            })?;

        // Only store the finalized approvals if they are different from the original ones.
        if original_deploy.approvals() != finalized_approvals.as_ref() {
            let _ = txn.put_value(
                self.finalized_approvals_db,
                deploy_hash,
                finalized_approvals,
                true,
            )?;
            txn.commit()?;
        }
        Ok(())
    }

    /// Creates a serialized representation of a `FetchedOrNotFound` and the resulting message.
    ///
    /// If the given item is `Some`, returns a serialization of `FetchedOrNotFound::Fetched`. If
    /// enabled, the given serialization is also added to the in-memory pool.
    ///
    /// If the given item is `None`, returns a non-pooled serialization of
    /// `FetchedOrNotFound::NotFound`.
    fn update_pool_and_send<REv, T>(
        &self,
        effect_builder: EffectBuilder<REv>,
        sender: NodeId,
        serialized_id: &[u8],
        id: T::Id,
        opt_item: Option<T>,
    ) -> Result<Effects<Event>, FatalStorageError>
    where
        REv: From<NetworkRequest<Message>> + Send,
        T: Item,
    {
        let fetched_or_not_found = FetchedOrNotFound::from_opt(id, opt_item);
        let serialized = fetched_or_not_found
            .to_serialized()
            .map_err(FatalStorageError::StoredItemSerializationFailure)?;
        let shared: Arc<[u8]> = serialized.into();

        if self.enable_mem_deduplication && fetched_or_not_found.was_found() {
            let mut serialized_item_pool = self.serialized_item_pool.write()?;
            serialized_item_pool.put(serialized_id.into(), Arc::downgrade(&shared));
        }

        let message = Message::new_get_response_from_serialized(<T as Item>::TAG, shared);
        Ok(effect_builder.send_message(sender, message).ignore())
    }

    fn get_available_block_range<Tx: Transaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<AvailableBlockRange, FatalStorageError> {
        let low = self
            .indices
            .get_lowest_available_block_height(txn)?
            .unwrap_or_default();
        let high = self
            .indices
            .get_highest_available_block_height(txn)?
            .unwrap_or_default();
        match AvailableBlockRange::new(low, high) {
            Ok(range) => Ok(range),
            Err(error) => {
                error!(%error);
                Ok(AvailableBlockRange::default())
            }
        }
    }

    fn update_lowest_available_block_height(
        &self,
        new_height: u64,
    ) -> Result<(), FatalStorageError> {
        // We should update the value if
        // * it wasn't already stored, or
        // * the new value is lower than the available range at startup, or
        // * the new value is 2 or more higher than (after pruning due to hard-reset) the available
        //   range at startup, as that would mean there's a gap of at least one block with
        //   unavailable global state.
        {
            let mut txn = self.env.begin_ro_txn()?;
            let should_update = match self.indices.get_lowest_available_block_height(&mut txn)? {
                Some(height) => {
                    new_height < height || new_height > self.highest_block_at_startup + 1
                }
                None => true,
            };
            if !should_update {
                return Ok(());
            }

            if self
                .indices
                .get_from_block_height_index(&mut txn, new_height)?
                .is_none()
            {
                error!(
                    %new_height,
                    "failed to update lowest_available_block_height as not in height index"
                );
                // We don't need to return a fatal error here as an invalid
                // `lowest_available_block_height` is not a critical error.
                return Ok(());
            }
        }

        let mut txn = self.env.begin_rw_txn()?;
        self.indices
            .put_lowest_available_block_height(&mut txn, new_height)?;
        txn.commit()?;
        Ok(())
    }

    fn is_v1_block(&self, block_header: &BlockHeader) -> bool {
        block_header.hashing_algorithm_version(self.verifiable_chunked_hash_activation)
            == HashingAlgorithmVersion::V1
    }
}

/// Decodes an item's ID, typically from an incoming request.
fn decode_item_id<T: Item>(raw: &[u8]) -> Result<T::Id, GetRequestError> {
    bincode::deserialize(raw).map_err(GetRequestError::MalformedIncomingItemId)
}

fn should_move_storage_files_to_network_subdir(
    root: &Path,
    file_names: &[&str],
) -> Result<bool, FatalStorageError> {
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

        return Err(FatalStorageError::MissingStorageFiles {
            missing_files: files_not_found,
        });
    }

    Ok(should_move_files)
}

fn move_storage_files_to_network_subdir(
    root: &Path,
    subdir: &Path,
    file_names: &[&str],
) -> Result<(), FatalStorageError> {
    file_names
        .iter()
        .map(|file_name| {
            let source_path = root.join(file_name);
            let dest_path = subdir.join(file_name);
            fs::rename(&source_path, &dest_path).map_err(|original_error| {
                FatalStorageError::UnableToMoveFile {
                    source_path,
                    dest_path,
                    original_error,
                }
            })
        })
        .collect::<Result<Vec<_>, FatalStorageError>>()?;

    info!(
        "moved files: {:?} from: {:?} to: {:?}",
        file_names, root, subdir
    );
    Ok(())
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
            .storage
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        txn.get_value(self.storage.deploy_db, &deploy_hash)
            .expect("could not retrieve value from storage")
    }

    /// Directly returns a deploy metadata from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn get_deploy_metadata_by_hash(
        &self,
        deploy_hash: &DeployHash,
    ) -> Option<DeployMetadata> {
        let mut txn = self
            .storage
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.storage
            .get_deploy_metadata(&mut txn, deploy_hash)
            .expect("could not retrieve deploy metadata from storage")
    }

    /// Directly returns a deploy with finalized approvals from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn get_deploy_with_finalized_approvals_by_hash(
        &self,
        deploy_hash: &DeployHash,
    ) -> Option<DeployWithFinalizedApprovals> {
        let mut txn = self
            .storage
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.storage
            .get_deploy_with_finalized_approvals(&mut txn, deploy_hash)
            .expect("could not retrieve a deploy with finalized approvals from storage")
    }

    /// Reads all known deploy hashes from the internal store.
    ///
    /// # Panics
    ///
    /// Panics on any IO or db corruption error.
    pub(crate) fn get_all_deploy_hashes(&self) -> BTreeSet<DeployHash> {
        let txn = self
            .storage
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");

        let mut cursor = txn
            .open_ro_cursor(self.storage.deploy_db)
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
        txn: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<Block>, FatalStorageError> {
        let getter_id = BlockGetterId::SwitchBlock(era_id);
        self.storage.get_block(txn, getter_id)
    }

    /// Get the switch block for a specified era number in a read-only LMDB database transaction.
    ///
    /// # Panics
    ///
    /// Panics on any IO or db corruption error.
    pub(crate) fn transactional_get_switch_block_by_era_id(
        &self,
        switch_block_era_num: u64,
    ) -> Result<Option<Block>, FatalStorageError> {
        let mut read_only_lmdb_transaction = self
            .storage
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
        Ok(switch_block)
    }

    /// Retrieves a single block header by hash.
    pub fn read_block_header_by_hash(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_hash(*block_hash);
        let mut txn = self.storage.env.begin_ro_txn()?;
        Ok(self
            .storage
            .get_block_header(&mut txn, getter_id)?
            .map(|(_block_hash, header)| header))
    }

    /// Retrieves a single block header by height by looking it up in the index and returning it.
    pub fn read_block_header_by_height(
        &self,
        block_height: u64,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_height(block_height);
        let mut txn = self.storage.env.begin_ro_txn()?;
        Ok(self
            .storage
            .get_block_header(&mut txn, getter_id)?
            .map(|(_block_hash, header)| header))
    }

    /// Retrieves a single block by height by looking it up in the index and returning it.
    fn get_block_by_height(&self, block_height: u64) -> Result<Option<Block>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_height(block_height);
        let mut txn = self.storage.env.begin_ro_txn()?;
        self.storage.get_block(&mut txn, getter_id)
    }

    pub fn read_block_header_and_enough_sigs_by_height(
        &self,
        block_height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_height(block_height);
        let mut txn = self.storage.env.begin_ro_txn()?;
        self.storage
            .get_block_header_with_enough_sigs(&mut txn, getter_id)
    }

    pub fn read_block_and_enough_sigs_by_height(
        &self,
        block_height: u64,
    ) -> Result<Option<BlockWithMetadata>, FatalStorageError> {
        let getter_id = BlockGetterId::with_block_height(block_height);
        let mut txn = self.storage.env.begin_ro_txn()?;
        self.storage.get_block_with_enough_sigs(&mut txn, getter_id)
    }

    /// Retrieves the highest block header from the storage, if one exists.
    pub fn read_highest_block_header(&self) -> Result<Option<BlockHeader>, FatalStorageError> {
        let getter_id = BlockGetterId::HighestBlock;
        let mut txn = self.storage.env.begin_ro_txn()?;
        Ok(self
            .storage
            .get_block_header(&mut txn, getter_id)?
            .map(|(_block_hash, header)| header))
    }

    pub fn get_available_block_range(&self) -> Result<AvailableBlockRange, FatalStorageError> {
        let mut txn = self.storage.env.begin_ro_txn()?;
        self.storage.get_available_block_range(&mut txn)
    }
}
