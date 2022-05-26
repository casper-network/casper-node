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

mod error;
mod lmdb_ext;
mod object_pool;
#[cfg(test)]
mod tests;

#[cfg(test)]
use std::collections::BTreeSet;
use std::{
    collections::{btree_map::Entry, BTreeMap, HashSet},
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
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use static_assertions::const_assert;
#[cfg(test)]
use tempfile::TempDir;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
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
        AvailableBlockRange, Block, BlockAndDeploys, BlockBody, BlockHash, BlockHashAndHeight,
        BlockHeader, BlockHeaderWithMetadata, BlockSignatures, BlockWithMetadata, Deploy,
        DeployHash, DeployMetadata, DeployMetadataExt, DeployWithFinalizedApprovals,
        FinalizedApprovals, FinalizedApprovalsWithId, HashingAlgorithmVersion, Item,
        MerkleBlockBody, MerkleBlockBodyPart, MerkleLinkedListNode, NodeId,
    },
    utils::{display_error, FlattenResult, WithDir},
    NodeRng,
};
pub use error::FatalStorageError;
use error::GetRequestError;
use lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt};
use object_pool::ObjectPool;

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";
/// The key in the state store DB under which the storage component's
/// `lowest_available_block_height` field is persisted.
const KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT: &str = "lowest_available_block_height";

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
const MAX_DB_COUNT: u32 = 12;

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

/// Groups databases used by storage.
struct Databases {
    block_body_v1_db: Database,
    block_body_v2_db: Database,
    deploy_hashes_db: Database,
    transfer_hashes_db: Database,
    proposer_db: Database,
}

/// The inner storage component.
#[derive(DataSize, Debug)]
pub struct StorageInner {
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
    /// The finalized approvals database.
    #[data_size(skip)]
    finalized_approvals_db: Database,
    /// Highest block available when storage is constructed.
    highest_block_at_startup: u64,
    /// Various indices used by the component.
    #[data_size(skip)]
    indices: RwLock<Indices>,
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

/// Database indices used by the storage component.
#[derive(Clone, DataSize, Debug, Default)]
struct Indices {
    /// A map of block height to block ID.
    block_height_index: BTreeMap<u64, BlockHash>,
    /// A map of era ID to switch block ID.
    switch_block_era_id_index: BTreeMap<EraId, BlockHash>,
    /// A map of deploy hashes to hashes and heights of blocks containing them.
    deploy_hash_index: BTreeMap<DeployHash, BlockHashAndHeight>,
    /// The height of the highest block from which this node has an unbroken sequence of full
    /// blocks stored (and the corresponding global state).
    lowest_available_block_height: u64,
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
                        match storage
                            .handle_net_request_incoming::<REv>(effect_builder, incoming)
                        {
                            Ok(effects) => Ok(effects),
                            Err(GetRequestError::Fatal(fatal_error)) => Err(fatal_error),
                            Err(ref other_err) => {
                                warn!(sender=%incoming.sender, err=display_error(other_err), "error handling net request");
                                // We could still send the requestor a "not found" message, and could do
                                // so even in the fatal case, but it is safer to not do so at the
                                // moment, giving less surface area for possible amplification attacks.
                                Ok(Effects::new())
                            }
                        }
                    }
                    Event::StateStoreRequest(req) => storage
                        .handle_state_store_request::<REv>(effect_builder, req),
                }
            };

            // While the operation is ready to run, we do not want to spawn an arbitrarily large
            // number of synchronous background tasks, especially since tokio does not put
            // meaningful bounds on these by default. For this reason, we limit the maximum number
            // of blocking tasks using a semaphore.
            let outcome = match sync_task_limiter.acquire().await {
                Ok(_permit) =>{
                    tokio::task::spawn_blocking(perform_op).await.map_err(FatalStorageError::FailedToJoinBackgroundTask).flatten_result()
                },
                Err(err) => Err(
                    FatalStorageError::SemaphoreClosedUnexpectedly(err),
                ),
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
                },
                Err(ref err) => {
                    // Any error is turned into a fatal effect, the component itself does not panic.
                    fatal!(effect_builder, "storage error: {}", display_error(err)).await;
                    Multiple::new()
                }
            }
        }.ignore()
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
        self.storage
            .read_switch_block_header_by_era_id(switch_block_era_id)
    }

    pub(crate) fn root_path(&self) -> &Path {
        self.storage.root_path()
    }

    // Note: Methods on `Storage` other than new should disappear and be replaced by an accessor
    // that exposes `StorageInner`, once the latter becomes the publically shared storage object.

    /// Reads a block from storage.
    #[inline]
    pub fn read_block(&self, block_hash: &BlockHash) -> Result<Option<Block>, FatalStorageError> {
        self.storage.read_block(block_hash)
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    #[inline]
    pub fn read_block_by_height(&self, height: u64) -> Result<Option<Block>, FatalStorageError> {
        self.storage.read_block_by_height(height)
    }

    /// Gets the highest block.
    pub fn read_highest_block(&self) -> Result<Option<Block>, FatalStorageError> {
        let mut tx = self.storage.env.begin_ro_txn()?;
        let indices = self.storage.indices.read()?;
        self.storage.get_highest_block(&mut tx, &indices)
    }

    /// Directly returns a deploy from internal store.
    #[inline]
    pub fn read_deploy_by_hash(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<Deploy>, FatalStorageError> {
        self.storage.read_deploy_by_hash(deploy_hash)
    }

    /// Put a single deploy into storage.
    #[inline]
    pub fn put_deploy(&self, deploy: &Deploy) -> Result<bool, FatalStorageError> {
        self.storage.put_deploy(deploy)
    }

    /// Write a block to storage.
    #[inline]
    pub fn write_block(&self, block: &Block) -> Result<bool, FatalStorageError> {
        self.storage.write_block(block)
    }
}

impl StorageInner {
    /// Creates a new inner storage.
    #[allow(clippy::too_many_arguments)]
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
                // Disable read-ahead. Our data is not storead/read in sequence that would benefit from the read-ahead.
                | EnvironmentFlags::NO_READAHEAD,
            )
            // We need at least `max_sync_tasks` readers, add an additional 8 for unforseen external
            // reads (not likely, but it does not hurt to increase this limit).
            .set_max_readers(config.max_sync_tasks as u32 + 8)
            .set_max_dbs(MAX_DB_COUNT)
            .set_map_size(total_size)
            .open(&root.join(STORAGE_DB_FILENAME))?;

        let block_header_db = env.create_db(Some("block_header"), DatabaseFlags::empty())?;
        let block_metadata_db = env.create_db(Some("block_metadata"), DatabaseFlags::empty())?;
        let deploy_db = env.create_db(Some("deploys"), DatabaseFlags::empty())?;
        let deploy_metadata_db = env.create_db(Some("deploy_metadata"), DatabaseFlags::empty())?;
        let transfer_db = env.create_db(Some("transfer"), DatabaseFlags::empty())?;
        let state_store_db = env.create_db(Some("state_store"), DatabaseFlags::empty())?;
        let finalized_approvals_db =
            env.create_db(Some("finalized_approvals"), DatabaseFlags::empty())?;
        let block_body_v1_db = env.create_db(Some("block_body"), DatabaseFlags::empty())?;
        let block_body_v2_db = env.create_db(Some("block_body_merkle"), DatabaseFlags::empty())?;
        let deploy_hashes_db = env.create_db(Some("deploy_hashes"), DatabaseFlags::empty())?;
        let transfer_hashes_db = env.create_db(Some("transfer_hashes"), DatabaseFlags::empty())?;
        let proposer_db = env.create_db(Some("proposers"), DatabaseFlags::empty())?;

        // We now need to restore the block-height index. Log messages allow timing here.
        info!("reindexing block store");
        let mut indices = Indices::default();
        let mut block_txn = env.begin_rw_txn()?;
        let mut cursor = block_txn.open_rw_cursor(block_header_db)?;

        let mut deleted_block_hashes = HashSet::new();
        let mut deleted_block_body_hashes_v1 = HashSet::new();
        let mut deleted_deploy_hashes = HashSet::<DeployHash>::new();

        let databases = Databases {
            block_body_v1_db,
            block_body_v2_db,
            deploy_hashes_db,
            transfer_hashes_db,
            proposer_db,
        };

        // Note: `iter_start` has an undocumented panic if called on an empty database. We rely on
        //       the iterator being at the start when created.
        for (_, raw_val) in cursor.iter() {
            let mut body_txn = env.begin_ro_txn()?;
            let block_header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
            let (maybe_block_body, is_v1) = get_body_for_block_header(
                &mut body_txn,
                &block_header,
                &databases,
                verifiable_chunked_hash_activation,
            );
            if let Some(invalid_era) = hard_reset_to_start_of_era {
                // Remove blocks that are in to-be-upgraded eras, but have obsolete protocol
                // versions - they were most likely created before the upgrade and should be
                // reverted.
                if block_header.era_id() >= invalid_era
                    && block_header.protocol_version() < protocol_version
                {
                    let _ = deleted_block_hashes
                        .insert(block_header.hash(verifiable_chunked_hash_activation));

                    if let Some(block_body) = maybe_block_body? {
                        deleted_deploy_hashes.extend(block_body.deploy_hashes());
                        deleted_deploy_hashes.extend(block_body.transfer_hashes());
                    }

                    if is_v1 {
                        let _ = deleted_block_body_hashes_v1.insert(*block_header.body_hash());
                    }

                    cursor.del(WriteFlags::empty())?;
                    continue;
                }
            }

            insert_to_block_header_indices(
                &mut indices,
                &block_header,
                verifiable_chunked_hash_activation,
            )?;

            if let Some(block_body) = maybe_block_body? {
                insert_to_deploy_index(
                    &mut indices.deploy_hash_index,
                    block_header.hash(verifiable_chunked_hash_activation),
                    &block_body,
                    block_header.height(),
                )?;
            }
        }
        info!("block store reindexing complete");
        drop(cursor);
        block_txn.commit()?;

        indices.lowest_available_block_height = {
            let mut txn = env.begin_ro_txn()?;
            txn.get_value_bytesrepr(state_store_db, &KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT)?
                .unwrap_or_default()
        };
        let highest_block_at_startup = indices
            .block_height_index
            .keys()
            .last()
            .copied()
            .unwrap_or_default();
        if let Err(error) = AvailableBlockRange::new(
            indices.lowest_available_block_height,
            highest_block_at_startup,
        ) {
            error!(%error);
            indices.lowest_available_block_height = highest_block_at_startup;
        }

        let deleted_block_hashes_raw = deleted_block_hashes.iter().map(BlockHash::as_ref).collect();

        initialize_block_body_v1_db(
            &env,
            &block_header_db,
            &block_body_v1_db,
            &deleted_block_body_hashes_v1
                .iter()
                .map(Digest::as_ref)
                .collect(),
        )?;

        initialize_block_metadata_db(&env, &block_metadata_db, &deleted_block_hashes_raw)?;
        initialize_deploy_metadata_db(&env, &deploy_metadata_db, &deleted_deploy_hashes)?;

        Ok(Self {
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
            finalized_approvals_db,
            highest_block_at_startup,
            indices: RwLock::new(indices),
            enable_mem_deduplication: config.enable_mem_deduplication,
            serialized_item_pool: RwLock::new(ObjectPool::new(config.mem_pool_prune_interval)),
            finality_threshold_fraction,
            last_emergency_restart,
            verifiable_chunked_hash_activation,
        })
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
                let bytes = self.read_state_store(&key)?;
                Ok(responder.respond(bytes).ignore())
            }
        }
    }

    /// Reads from the state storage DB.
    /// If key is non-empty, returns bytes from under the key. Otherwise returns `Ok(None)`.
    /// May also fail with storage errors.
    pub(crate) fn read_state_store<K>(&self, key: &K) -> Result<Option<Vec<u8>>, FatalStorageError>
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

    /// Returns the path to the storage folder.
    pub(crate) fn root_path(&self) -> &Path {
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
                // We found an item in the pool. We can short-circuit all
                // deserialization/serialization and return the canned item
                // immediately.
                let found = Message::new_get_response_from_serialized(
                    incoming.message.tag(),
                    serialized_item,
                );
                return Ok(effect_builder.send_message(incoming.sender, found).ignore());
            }
        }

        match incoming.message {
            NetRequest::Deploy(ref serialized_id) => {
                let id = decode_item_id::<Deploy>(serialized_id)?;
                let opt_item = self.get_deploy(id).map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    id,
                    opt_item,
                )?)
            }
            NetRequest::FinalizedApprovals(ref serialized_id) => {
                let id = decode_item_id::<FinalizedApprovalsWithId>(serialized_id)?;
                let opt_item = self
                    .env
                    .begin_ro_txn()
                    .map_err(Into::into)
                    .and_then(|mut tx| {
                        self.get_deploy_with_finalized_approvals(&mut tx, &id)
                            .map_err(FatalStorageError::from)
                    })?
                    .map(|deploy| {
                        FinalizedApprovalsWithId::new(
                            id,
                            FinalizedApprovals::new(deploy.into_naive().approvals().clone()),
                        )
                    });

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    id,
                    opt_item,
                )?)
            }
            NetRequest::Block(ref serialized_id) => {
                let id = decode_item_id::<Block>(serialized_id)?;
                let opt_item = self.read_block(&id).map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    id,
                    opt_item,
                )?)
            }
            NetRequest::GossipedAddress(_) => Err(GetRequestError::GossipedAddressNotGettable),
            NetRequest::BlockAndMetadataByHeight(ref serialized_id) => {
                let item_id = decode_item_id::<BlockWithMetadata>(serialized_id)?;
                let opt_item = self
                    .read_block_and_sufficient_finality_signatures_by_height(item_id)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    item_id,
                    opt_item,
                )?)
            }
            NetRequest::BlockHeaderByHash(ref serialized_id) => {
                let item_id = decode_item_id::<BlockHeader>(serialized_id)?;
                let opt_item = self
                    .read_block_header_by_hash(&item_id)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    item_id,
                    opt_item,
                )?)
            }
            NetRequest::BlockHeaderAndFinalitySignaturesByHeight(ref serialized_id) => {
                let item_id = decode_item_id::<BlockHeaderWithMetadata>(serialized_id)?;
                let opt_item = self
                    .read_block_header_and_sufficient_finality_signatures_by_height(item_id)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    item_id,
                    opt_item,
                )?)
            }
            NetRequest::BlockAndDeploys(ref serialized_id) => {
                let item_id = decode_item_id::<BlockAndDeploys>(serialized_id)?;
                let opt_item = self
                    .read_block_and_deploys_by_hash(item_id)
                    .map_err(FatalStorageError::from)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    item_id,
                    opt_item,
                )?)
            }
        }
    }

    /// Handles a storage request.
    fn handle_storage_request<REv>(
        &self,
        req: StorageRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
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
            StorageRequest::GetHighestBlock { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let indices = self.indices.read()?;
                responder
                    .respond(self.get_highest_block(&mut txn, &indices)?)
                    .ignore()
            }
            StorageRequest::GetHighestBlockHeader { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let indices = self.indices.read()?;
                responder
                    .respond(self.get_highest_block_header(&mut txn, &indices)?)
                    .ignore()
            }
            StorageRequest::GetSwitchBlockHeaderAtEraId { era_id, responder } => {
                let indices = self.indices.read()?;
                let mut tx = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_switch_block_header_by_era_id(&mut tx, &indices, era_id)?)
                    .ignore()
            }
            StorageRequest::GetBlockHeaderForDeploy {
                deploy_hash,
                responder,
            } => {
                let mut tx = self.env.begin_ro_txn()?;
                let indices = self.indices.read()?;
                responder
                    .respond(self.get_block_header_by_deploy_hash(
                        &mut tx,
                        &indices,
                        deploy_hash,
                    )?)
                    .ignore()
            }
            StorageRequest::GetBlockHeader {
                block_hash,
                only_from_available_block_range,
                responder,
            } => responder
                .respond(self.get_single_block_header_restricted(
                    &mut self.env.begin_ro_txn()?,
                    &block_hash,
                    only_from_available_block_range,
                )?)
                .ignore(),
            StorageRequest::CheckBlockHeaderExistence {
                block_height,
                responder,
            } => {
                let indices = self.indices.read()?;
                responder
                    .respond(self.block_header_exists(&indices, block_height))
                    .ignore()
            }
            StorageRequest::GetBlockTransfers {
                block_hash,
                responder,
            } => responder
                .respond(self.get_transfers(&mut self.env.begin_ro_txn()?, &block_hash)?)
                .ignore(),
            StorageRequest::PutDeploy { deploy, responder } => {
                responder.respond(self.put_deploy(&*deploy)?).ignore()
            }
            StorageRequest::GetDeploys {
                deploy_hashes,
                responder,
            } => responder
                .respond(self.get_deploys_with_finalized_approvals(
                    &mut self.env.begin_ro_txn()?,
                    deploy_hashes.as_slice(),
                )?)
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

                let deploy = {
                    let opt_deploy =
                        self.get_deploy_with_finalized_approvals(&mut txn, &deploy_hash)?;

                    if let Some(deploy) = opt_deploy {
                        deploy
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
                };

                // Missing metadata is filled using a default.
                let metadata_ext: DeployMetadataExt =
                    if let Some(metadata) = self.get_deploy_metadata(&mut txn, &deploy_hash)? {
                        metadata.into()
                    } else {
                        let indices = self.indices.read()?;
                        if let Some(block_hash_and_height) =
                            self.get_block_hash_and_height_by_deploy_hash(&indices, deploy_hash)?
                        {
                            block_hash_and_height.into()
                        } else {
                            DeployMetadataExt::Empty
                        }
                    };

                responder.respond(Some((deploy, metadata_ext))).ignore()
            }
            StorageRequest::GetBlockAndMetadataByHash {
                block_hash,
                only_from_available_block_range,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;

                let block: Block =
                    if let Some(block) = self.get_single_block(&mut txn, &block_hash)? {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    };

                if !(self.should_return_block(block.height(), only_from_available_block_range)?) {
                    return Ok(responder.respond(None).ignore());
                }

                // Check that the hash of the block retrieved is correct.
                if block_hash != *block.hash() {
                    error!(
                        queried_block_hash = ?block_hash,
                        actual_block_hash = ?block.hash(),
                        "block not stored under hash"
                    );
                    debug_assert_eq!(&block_hash, block.hash());
                    return Ok(responder.respond(None).ignore());
                }
                let finality_signatures =
                    match self.get_finality_signatures(&mut txn, &block_hash)? {
                        Some(signatures) => signatures,
                        None => BlockSignatures::new(block_hash, block.header().era_id()),
                    };
                if finality_signatures.verify().is_err() {
                    error!(?block, "invalid finality signatures for block");
                    debug_assert!(finality_signatures.verify().is_ok());
                    return Ok(responder.respond(None).ignore());
                }
                responder
                    .respond(Some(BlockWithMetadata {
                        block,
                        finality_signatures,
                    }))
                    .ignore()
            }
            StorageRequest::GetBlockAndSufficientFinalitySignaturesByHeight {
                block_height,
                responder,
            } => responder
                .respond(
                    self.read_block_and_sufficient_finality_signatures_by_height(block_height)?,
                )
                .ignore(),
            StorageRequest::GetBlockAndMetadataByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            } => {
                if !(self.should_return_block(block_height, only_from_available_block_range)?) {
                    return Ok(responder.respond(None).ignore());
                }

                let mut txn = self.env.begin_ro_txn()?;

                let block: Block = {
                    let indices = self.indices.read()?;
                    if let Some(block) =
                        self.get_block_by_height(&mut txn, &indices, block_height)?
                    {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
                };

                let hash = block.hash();
                let finality_signatures = match self.get_finality_signatures(&mut txn, hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(*hash, block.header().era_id()),
                };
                responder
                    .respond(Some(BlockWithMetadata {
                        block,
                        finality_signatures,
                    }))
                    .ignore()
            }
            StorageRequest::GetHighestBlockWithMetadata { responder } => {
                let mut txn = self.env.begin_ro_txn()?;

                let highest_block: Block = {
                    let indices = self.indices.read()?;
                    if let Some(block) = indices
                        .block_height_index
                        .keys()
                        .last()
                        .and_then(|&height| {
                            self.get_block_by_height(&mut txn, &indices, height)
                                .transpose()
                        })
                        .transpose()?
                    {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
                };
                let hash = highest_block.hash();
                let finality_signatures = match self.get_finality_signatures(&mut txn, hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(*hash, highest_block.header().era_id()),
                };
                responder
                    .respond(Some(BlockWithMetadata {
                        block: highest_block,
                        finality_signatures,
                    }))
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
            StorageRequest::GetFinalizedBlocks { ttl, responder } => {
                responder.respond(self.get_finalized_blocks(ttl)?).ignore()
            }
            StorageRequest::GetBlockHeaderByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            } => {
                let indices = self.indices.read()?;
                let result = self.get_block_header_by_height_restricted(
                    &mut self.env.begin_ro_txn()?,
                    &indices,
                    block_height,
                    only_from_available_block_range,
                )?;
                responder.respond(result).ignore()
            }
            StorageRequest::GetBlockHeaderAndSufficientFinalitySignaturesByHeight {
                block_height,
                responder,
            } => {
                let indices = self.indices.read()?;
                let result = self.get_block_header_and_sufficient_finality_signatures_by_height(
                    &mut self.env.begin_ro_txn()?,
                    &indices,
                    block_height,
                )?;
                responder.respond(result).ignore()
            }
            StorageRequest::PutBlockHeader {
                block_header,
                responder,
            } => {
                let mut txn = self.env.begin_rw_txn()?;
                if !txn.put_value(
                    self.block_header_db,
                    &block_header.hash(self.verifiable_chunked_hash_activation),
                    &block_header,
                    false,
                )? {
                    error!(
                        ?block_header,
                        "Could not insert block header (maybe already inserted?)",
                    );
                    txn.abort();
                    return Ok(responder.respond(false).ignore());
                }

                {
                    let mut indices = self.indices.write()?;
                    insert_to_block_header_indices(
                        &mut indices,
                        &block_header,
                        self.verifiable_chunked_hash_activation,
                    )?;
                }

                txn.commit()?;
                responder.respond(true).ignore()
            }
            StorageRequest::UpdateLowestAvailableBlockHeight { height, responder } => {
                self.update_lowest_available_block_height(height)?;
                responder.respond(()).ignore()
            }
            StorageRequest::GetAvailableBlockRange { responder } => responder
                .respond(self.get_available_block_range()?)
                .ignore(),
            StorageRequest::StoreFinalizedApprovals {
                ref deploy_hash,
                ref finalized_approvals,
                responder,
            } => responder
                .respond(self.store_finalized_approvals(deploy_hash, finalized_approvals)?)
                .ignore(),
            StorageRequest::PutBlockAndDeploys { block, responder } => responder
                .respond(self.put_block_and_deploys(&*block)?)
                .ignore(),
            StorageRequest::GetBlockAndDeploys {
                block_hash,
                responder,
            } => responder
                .respond(self.read_block_and_deploys_by_hash(block_hash)?)
                .ignore(),
        })
    }

    /// Put a single deploy into storage.
    pub fn put_deploy(&self, deploy: &Deploy) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let outcome = txn.put_value(self.deploy_db, deploy.id(), deploy, false)?;
        txn.commit()?;
        Ok(outcome)
    }

    /// Puts block and its deploys into storage.
    ///
    /// Returns `Ok` only if the block and all deploys were successfully written.
    pub fn put_block_and_deploys(
        &self,
        block_and_deploys: &BlockAndDeploys,
    ) -> Result<(), FatalStorageError> {
        let BlockAndDeploys { block, deploys } = block_and_deploys;

        block.verify(self.verifiable_chunked_hash_activation)?;
        let mut txn = self.env.begin_rw_txn()?;
        match self.write_validated_block(&mut txn, block) {
            Ok(true) => {}
            Ok(false) => {
                txn.abort();
                return Err(FatalStorageError::FailedToOverwriteBlock);
            }
            Err(error) => {
                txn.abort();
                return Err(error);
            }
        }

        for deploy in deploys {
            let _ = txn.put_value(self.deploy_db, deploy.id(), deploy, false)?;
        }
        txn.commit()?;

        Ok(())
    }

    /// Retrieves a block by hash.
    pub fn read_block(&self, block_hash: &BlockHash) -> Result<Option<Block>, FatalStorageError> {
        self.get_single_block(&mut self.env.begin_ro_txn()?, block_hash)
    }

    /// Writes a block to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    pub fn write_block(&self, block: &Block) -> Result<bool, FatalStorageError> {
        // Validate the block prior to inserting it into the database
        block.verify(self.verifiable_chunked_hash_activation)?;
        let mut txn = self.env.begin_rw_txn()?;
        let result = self.write_validated_block(&mut txn, block);
        match &result {
            Ok(false) | Err(_) => txn.abort(),
            Ok(true) => txn.commit()?,
        }
        result
    }

    /// Writes a block which has already been verified to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write_validated_block(
        &self,
        txn: &mut RwTransaction,
        block: &Block,
    ) -> Result<bool, FatalStorageError> {
        {
            let block_body_hash = block.header().body_hash();
            let block_body = block.body();
            let success = match block
                .header()
                .hashing_algorithm_version(self.verifiable_chunked_hash_activation)
            {
                HashingAlgorithmVersion::V1 => {
                    self.put_single_block_body_v1(txn, block_body_hash, block_body)?
                }
                HashingAlgorithmVersion::V2 => self.put_single_block_body_v2(txn, block_body)?,
            };
            if !success {
                error!("Could not insert body for: {}", block);
                return Ok(false);
            }
        }

        if !txn.put_value(self.block_header_db, block.hash(), block.header(), true)? {
            error!("Could not insert block header for block: {}", block);
            return Ok(false);
        }

        {
            let mut indices = self.indices.write()?;
            insert_to_block_header_indices(
                &mut indices,
                block.header(),
                self.verifiable_chunked_hash_activation,
            )?;
            insert_to_deploy_index(
                &mut indices.deploy_hash_index,
                block.header().hash(self.verifiable_chunked_hash_activation),
                block.body(),
                block.header().height(),
            )?;
        }
        Ok(true)
    }

    /// Get the switch block header for a specified [`EraID`].
    pub(crate) fn read_switch_block_header_by_era_id(
        &self,
        switch_block_era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut tx = self.env.begin_ro_txn()?;
        let indices = self.indices.read()?;
        self.get_switch_block_header_by_era_id(&mut tx, &indices, switch_block_era_id)
    }

    /// Retrieves a block header by height.
    /// Returns `None` if they are less than the fault tolerance threshold, or if the block is from
    /// before the most recent emergency upgrade.
    pub fn read_block_header_and_sufficient_finality_signatures_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let indices = self.indices.read()?;
        let maybe_block_header_and_finality_signatures = self
            .get_block_header_and_sufficient_finality_signatures_by_height(
                &mut txn, &indices, height,
            )?;
        drop(txn);
        Ok(maybe_block_header_and_finality_signatures)
    }

    /// Retrieves a block by height.
    /// Returns `None` if they are less than the fault tolerance threshold, or if the block is from
    /// before the most recent emergency upgrade.
    pub fn read_block_and_sufficient_finality_signatures_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockWithMetadata>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let indices = self.indices.read()?;
        let maybe_block_and_finality_signatures = self
            .get_block_and_sufficient_finality_signatures_by_height(&mut txn, &indices, height)?;
        drop(txn);
        Ok(maybe_block_and_finality_signatures)
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    pub fn read_block_by_height(&self, height: u64) -> Result<Option<Block>, FatalStorageError> {
        let indices = self.indices.read()?;
        self.get_block_by_height(&mut self.env.begin_ro_txn()?, &indices, height)
    }

    /// Retrieves single block and all of its deploys.
    /// If any of the deploys can't be found, returns `Ok(None)`.
    pub fn read_block_and_deploys_by_hash(
        &self,
        hash: BlockHash,
    ) -> Result<Option<BlockAndDeploys>, FatalStorageError> {
        let block = self.read_block(&hash)?;
        match block {
            None => Ok(None),
            Some(block) => {
                let deploy_hashes = block
                    .deploy_hashes()
                    .iter()
                    .chain(block.transfer_hashes().iter());
                let deploys_count = block.deploy_hashes().len() + block.transfer_hashes().len();
                Ok(self
                    .read_deploys(deploys_count, deploy_hashes)?
                    .map(|deploys| BlockAndDeploys { block, deploys }))
            }
        }
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    fn get_block_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        indices: &Indices,
        height: u64,
    ) -> Result<Option<Block>, FatalStorageError> {
        indices
            .block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves single switch block header by era ID by looking it up in the index and returning
    /// it.
    fn get_switch_block_header_by_era_id<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        indices: &Indices,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        debug!(switch_block_era_id_index = ?indices.switch_block_era_id_index);
        indices
            .switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| self.get_single_block_header(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves a single block header by deploy hash by looking it up in the index and returning
    /// it.
    fn get_block_header_by_deploy_hash<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        indices: &Indices,
        deploy_hash: DeployHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        indices
            .deploy_hash_index
            .get(&deploy_hash)
            .and_then(|block_hash_and_height| {
                self.get_single_block_header(tx, &block_hash_and_height.block_hash)
                    .transpose()
            })
            .transpose()
    }

    /// Retrieves the block hash and height for a deploy hash by looking it up in the index
    /// and returning it.
    fn get_block_hash_and_height_by_deploy_hash(
        &self,
        indices: &Indices,
        deploy_hash: DeployHash,
    ) -> Result<Option<BlockHashAndHeight>, FatalStorageError> {
        Ok(indices.deploy_hash_index.get(&deploy_hash).copied())
    }

    /// Retrieves the highest block from storage, if one exists. May return an LMDB error.
    fn get_highest_block<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        indices: &Indices,
    ) -> Result<Option<Block>, FatalStorageError> {
        indices
            .block_height_index
            .keys()
            .last()
            .and_then(|&height| self.get_block_by_height(txn, indices, height).transpose())
            .transpose()
    }

    /// Retrieves the highest block header from storage, if one exists. May return an LMDB error.
    fn get_highest_block_header<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        indices: &Indices,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        indices
            .block_height_index
            .iter()
            .last()
            .and_then(|(_, hash_of_highest_block)| {
                self.get_single_block_header(txn, hash_of_highest_block)
                    .transpose()
            })
            .transpose()
    }

    /// Returns vector blocks that satisfy the predicate, starting from the latest one and following
    /// the ancestry chain.
    fn get_blocks_while<F, Tx: Transaction>(
        &self,
        txn: &mut Tx,
        indices: &Indices,
        predicate: F,
    ) -> Result<Vec<Block>, FatalStorageError>
    where
        F: Fn(&Block) -> bool,
    {
        let mut next_block = self.get_highest_block(txn, indices)?;
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

    /// Returns the vector of blocks that could still have deploys whose TTL hasn't expired yet.
    fn get_finalized_blocks(&self, ttl: TimeDiff) -> Result<Vec<Block>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let indices = self.indices.read()?;
        // We're interested in deploys whose TTL hasn't expired yet.
        let ttl_not_expired = |block: &Block| block.timestamp().elapsed() < ttl;
        self.get_blocks_while(&mut txn, &indices, ttl_not_expired)
    }

    /// Retrieves a single block header in a given transaction from storage
    /// respecting the possible restriction on whether the block
    /// should be present in the available blocks index.
    fn get_single_block_header_restricted<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header: BlockHeader = match tx.get_value(self.block_header_db, &block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };

        if !(self.should_return_block(block_header.height(), only_from_available_block_range)?) {
            return Ok(None);
        }

        self.validate_block_header_hash(&block_header, block_hash)?;
        Ok(Some(block_header))
    }

    fn get_block_header_by_height_restricted<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        indices: &Indices,
        block_height: u64,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_hash = match indices.block_height_index.get(&block_height) {
            None => return Ok(None),
            Some(block_hash) => block_hash,
        };

        self.get_single_block_header_restricted(tx, block_hash, only_from_available_block_range)
    }

    /// Retrieves a single block header in a given transaction from storage.
    fn get_single_block_header<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header: BlockHeader = match tx.get_value(self.block_header_db, &block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        self.validate_block_header_hash(&block_header, block_hash)?;
        Ok(Some(block_header))
    }

    /// Validates the block header hash against the expected block hash.
    fn validate_block_header_hash(
        &self,
        block_header: &BlockHeader,
        block_hash: &BlockHash,
    ) -> Result<(), FatalStorageError> {
        let found_block_header_hash = block_header.hash(self.verifiable_chunked_hash_activation);
        if found_block_header_hash != *block_hash {
            return Err(FatalStorageError::BlockHeaderNotStoredUnderItsHash {
                queried_block_hash_bytes: block_hash.as_ref().to_vec(),
                found_block_header_hash,
                block_header: Box::new(block_header.clone()),
            });
        };
        Ok(())
    }

    /// Checks whether a block at the given height exists in the block height index (and, since the
    /// index is initialized on startup based on the actual contents of storage, if it exists in
    /// storage).
    fn block_header_exists(&self, indices: &Indices, block_height: u64) -> bool {
        indices.block_height_index.contains_key(&block_height)
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
    pub(crate) fn read_block_header_by_hash(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
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
    ) -> Result<Option<Block>, FatalStorageError> {
        let block_header: BlockHeader = match self.get_single_block_header(tx, block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let (maybe_block_body, _) = get_body_for_block_header(
            tx,
            &block_header,
            &Databases {
                block_body_v1_db: self.block_body_v1_db,
                block_body_v2_db: self.block_body_v2_db,
                deploy_hashes_db: self.deploy_hashes_db,
                transfer_hashes_db: self.transfer_hashes_db,
                proposer_db: self.proposer_db,
            },
            self.verifiable_chunked_hash_activation,
        );
        let block_body = match maybe_block_body? {
            Some(block_body) => block_body,
            None => {
                info!(
                    ?block_header,
                    "retrieved block header but block body is missing from database"
                );
                return Ok(None);
            }
        };
        let block = Block::new_from_header_and_body(
            block_header,
            block_body,
            self.verifiable_chunked_hash_activation,
        )?;
        Ok(Some(block))
    }

    /// Retrieves a set of deploys from storage, along with their potential finalized approvals.
    fn get_deploys_with_finalized_approvals<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        deploy_hashes: &[DeployHash],
    ) -> Result<SmallVec<[Option<DeployWithFinalizedApprovals>; 1]>, LmdbExtError> {
        deploy_hashes
            .iter()
            .map(|deploy_hash| self.get_deploy_with_finalized_approvals(tx, deploy_hash))
            .collect()
    }

    /// Retrieves a single deploy along with its finalized approvals from storage
    fn get_deploy_with_finalized_approvals<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        deploy_hash: &DeployHash,
    ) -> Result<Option<DeployWithFinalizedApprovals>, LmdbExtError> {
        let maybe_original_deploy = tx.get_value(self.deploy_db, deploy_hash)?;
        if let Some(deploy) = maybe_original_deploy {
            let maybe_finalized_approvals =
                tx.get_value(self.finalized_approvals_db, deploy_hash)?;
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
        tx: &mut Tx,
        deploy_hash: &DeployHash,
    ) -> Result<Option<DeployMetadata>, FatalStorageError> {
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
    ) -> Result<Option<Vec<Transfer>>, FatalStorageError> {
        Ok(tx.get_value(self.transfer_db, block_hash)?)
    }

    /// Retrieves finality signatures for a block with a given block hash
    fn get_finality_signatures<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, FatalStorageError> {
        Ok(tx.get_value(self.block_metadata_db, block_hash)?)
    }

    /// Retrieves single block header by height by looking it up in the index and returning it;
    /// returns `None` if they are less than the fault tolerance threshold, or if the block is from
    /// before the most recent emergency upgrade.
    fn get_block_and_sufficient_finality_signatures_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        indices: &Indices,
        height: u64,
    ) -> Result<Option<BlockWithMetadata>, FatalStorageError> {
        let BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        } = match self
            .get_block_header_and_sufficient_finality_signatures_by_height(tx, indices, height)?
        {
            None => return Ok(None),
            Some(block_header_with_metadata) => block_header_with_metadata,
        };
        let (maybe_block_body, _) = get_body_for_block_header(
            tx,
            &block_header,
            &Databases {
                block_body_v1_db: self.block_body_v1_db,
                block_body_v2_db: self.block_body_v2_db,
                deploy_hashes_db: self.deploy_hashes_db,
                transfer_hashes_db: self.transfer_hashes_db,
                proposer_db: self.proposer_db,
            },
            self.verifiable_chunked_hash_activation,
        );
        if let Some(block_body) = maybe_block_body? {
            Ok(Some(BlockWithMetadata {
                block: Block::new_from_header_and_body(
                    block_header,
                    block_body,
                    self.verifiable_chunked_hash_activation,
                )?,
                finality_signatures: block_signatures,
            }))
        } else {
            debug!(?block_header, "Missing block body for header");
            Ok(None)
        }
    }

    /// Directly returns a deploy from internal store.
    pub fn read_deploy_by_hash(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<Deploy>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        Ok(txn.get_value(self.deploy_db, &deploy_hash)?)
    }

    /// Directly returns all deploys or None if any is missing.
    pub fn read_deploys<'a, I: Iterator<Item = &'a DeployHash> + 'a>(
        &self,
        deploys_count: usize,
        deploy_hashes: I,
    ) -> Result<Option<Vec<Deploy>>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let mut result = Vec::with_capacity(deploys_count);
        for deploy_hash in deploy_hashes {
            match txn.get_value(self.deploy_db, deploy_hash)? {
                Some(deploy) => result.push(deploy),
                None => return Ok(None),
            }
        }
        Ok(Some(result))
    }

    /// Stores a set of finalized approvals.
    pub fn store_finalized_approvals(
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

    /// Retrieves single block header by height by looking it up in the index and returning it;
    /// returns `None` if they are less than the fault tolerance threshold, or if the block is from
    /// before the most recent emergency upgrade.
    fn get_block_header_and_sufficient_finality_signatures_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        indices: &Indices,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let block_hash = match indices.block_height_index.get(&height) {
            None => return Ok(None),
            Some(block_hash) => block_hash,
        };
        let block_header = match self.get_single_block_header(tx, block_hash)? {
            None => return Ok(None),
            Some(block_header) => block_header,
        };
        let block_signatures =
            match self.get_sufficient_finality_signatures(tx, indices, &block_header)? {
                None => return Ok(None),
                Some(signatures) => signatures,
            };
        Ok(Some(BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        }))
    }

    /// Retrieves finality signatures for a block with a given block hash; returns `None` if they
    /// are less than the fault tolerance threshold or if the block is from before the most recent
    /// emergency upgrade.
    fn get_sufficient_finality_signatures<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        indices: &Indices,
        block_header: &BlockHeader,
    ) -> Result<Option<BlockSignatures>, FatalStorageError> {
        if let Some(last_emergency_restart) = self.last_emergency_restart {
            if block_header.era_id() <= last_emergency_restart {
                debug!(
                    ?block_header,
                    ?last_emergency_restart,
                    "finality signatures from before last emergency restart requested"
                );
                return Ok(None);
            }
        }
        let block_signatures = match self.get_finality_signatures(
            tx,
            &block_header.hash(self.verifiable_chunked_hash_activation),
        )? {
            None => return Ok(None),
            Some(block_signatures) => block_signatures,
        };
        let switch_block_hash = match indices
            .switch_block_era_id_index
            .get(&(block_header.era_id() - 1))
        {
            None => return Ok(None),
            Some(switch_block_hash) => switch_block_hash,
        };
        let switch_block_header = match self.get_single_block_header(tx, switch_block_hash)? {
            None => return Ok(None),
            Some(switch_block_header) => switch_block_header,
        };

        let validator_weights = match switch_block_header.next_era_validator_weights() {
            None => {
                return Err(FatalStorageError::InvalidSwitchBlock(Box::new(
                    switch_block_header,
                )))
            }
            Some(validator_weights) => validator_weights,
        };

        let block_signatures = consensus::get_minimal_set_of_signatures(
            validator_weights,
            self.finality_threshold_fraction,
            block_signatures,
        );

        // `block_signatures` is already an `Option`, which is `None` if there weren't enough
        // signatures to bring the total weight over the threshold.
        Ok(block_signatures)
    }

    /// Retrieves a deploy from the deploy store.
    pub fn get_deploy(&self, deploy_hash: DeployHash) -> Result<Option<Deploy>, LmdbExtError> {
        self.env
            .begin_ro_txn()
            .map_err(Into::into)
            .and_then(|mut tx| tx.get_value(self.deploy_db, &deploy_hash))
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

    /// Returns `true` if the storage should attempt to return a block. Depending on the
    /// `only_from_available_block_range` flag it should be unconditional or restricted by the
    /// available block range.
    fn should_return_block(
        &self,
        block_height: u64,
        only_from_available_block_range: bool,
    ) -> Result<bool, FatalStorageError> {
        if only_from_available_block_range {
            Ok(self.get_available_block_range()?.contains(block_height))
        } else {
            Ok(true)
        }
    }

    fn get_available_block_range(&self) -> Result<AvailableBlockRange, FatalStorageError> {
        let indices = self.indices.read()?;
        let high = indices
            .block_height_index
            .keys()
            .last()
            .copied()
            .unwrap_or_default();
        let low = indices.lowest_available_block_height;
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
        let should_update = {
            let mut txn = self.env.begin_ro_txn()?;
            match txn.get_value_bytesrepr::<_, u64>(
                self.state_store_db,
                &KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT,
            )? {
                Some(height) => {
                    new_height < height || new_height > self.highest_block_at_startup + 1
                }
                None => true,
            }
        };

        if !should_update {
            return Ok(());
        }

        let mut indices = self.indices.write()?;
        if indices.block_height_index.contains_key(&new_height) {
            let mut txn = self.env.begin_rw_txn()?;
            txn.put_value_bytesrepr::<_, u64>(
                self.state_store_db,
                &KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT,
                &new_height,
                true,
            )?;
            txn.commit()?;
            indices.lowest_available_block_height = new_height;
        } else {
            error!(
                %new_height,
                "failed to update lowest_available_block_height as not in height index"
            );
            // We don't need to return a fatal error here as an invalid
            // `lowest_available_block_height` is not a critical error.
        }

        Ok(())
    }
}

/// Decodes an item's ID, typically from an incoming request.
fn decode_item_id<T>(raw: &[u8]) -> Result<T::Id, GetRequestError>
where
    T: Item,
{
    bincode::deserialize(raw).map_err(GetRequestError::MalformedIncomingItemId)
}

/// Inserts the relevant entries to the two indices.
///
/// If a duplicate entry is encountered, neither index is updated and an error is returned.
fn insert_to_block_header_indices(
    indices: &mut Indices,
    block_header: &BlockHeader,
    verifiable_chunked_hash_activation: EraId,
) -> Result<(), FatalStorageError> {
    let block_hash = block_header.hash(verifiable_chunked_hash_activation);
    if let Some(first) = indices.block_height_index.get(&block_header.height()) {
        if *first != block_hash {
            return Err(FatalStorageError::DuplicateBlockIndex {
                height: block_header.height(),
                first: *first,
                second: block_hash,
            });
        }
    }

    if block_header.is_switch_block() {
        match indices
            .switch_block_era_id_index
            .entry(block_header.era_id())
        {
            Entry::Vacant(entry) => {
                let _ = entry.insert(block_hash);
            }
            Entry::Occupied(entry) => {
                if *entry.get() != block_hash {
                    return Err(FatalStorageError::DuplicateEraIdIndex {
                        era_id: block_header.era_id(),
                        first: *entry.get(),
                        second: block_hash,
                    });
                }
            }
        }
    }

    let _ = indices
        .block_height_index
        .insert(block_header.height(), block_hash);
    Ok(())
}

/// Inserts the relevant entries to the index.
///
/// If a duplicate entry is encountered, index is not updated and an error is returned.
fn insert_to_deploy_index(
    deploy_hash_index: &mut BTreeMap<DeployHash, BlockHashAndHeight>,
    block_hash: BlockHash,
    block_body: &BlockBody,
    block_height: u64,
) -> Result<(), FatalStorageError> {
    if let Some(hash) = block_body
        .deploy_hashes()
        .iter()
        .chain(block_body.transfer_hashes().iter())
        .find(|hash| {
            deploy_hash_index
                .get(hash)
                .map_or(false, |old_block_hash_and_height| {
                    old_block_hash_and_height.block_hash != block_hash
                })
        })
    {
        return Err(FatalStorageError::DuplicateDeployIndex {
            deploy_hash: *hash,
            first: deploy_hash_index[hash],
            second: BlockHashAndHeight::new(block_hash, block_height),
        });
    }

    for hash in block_body
        .deploy_hashes()
        .iter()
        .chain(block_body.transfer_hashes().iter())
    {
        deploy_hash_index.insert(*hash, BlockHashAndHeight::new(block_hash, block_height));
    }

    Ok(())
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

/// On-disk storage configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The path to the folder where any files created or read by the storage component will exist.
    ///
    /// If the folder doesn't exist, it and any required parents will be created.
    pub path: PathBuf,
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
    /// Maximum number of parallel synchronous storage tasks to spawn.
    max_sync_tasks: u16,
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
            enable_mem_deduplication: true,
            mem_pool_prune_interval: 4096,
            max_sync_tasks: 32,
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
        tx: &mut Tx,
        indices: &Indices,
        era_id: EraId,
    ) -> Result<Option<Block>, FatalStorageError> {
        indices
            .switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| self.storage.get_single_block(tx, block_hash).transpose())
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
    ) -> Result<Option<Block>, FatalStorageError> {
        let mut read_only_lmdb_transaction = self
            .storage
            .env
            .begin_ro_txn()
            .expect("Could not start read only transaction for lmdb");
        let indices = self.storage.indices.read()?;
        let switch_block = self
            .get_switch_block_by_era_id(
                &mut read_only_lmdb_transaction,
                &indices,
                EraId::from(switch_block_era_num),
            )
            .expect("LMDB panicked trying to get switch block");
        read_only_lmdb_transaction
            .commit()
            .expect("Could not commit transaction");
        Ok(switch_block)
    }

    /// Retrieves a single block header by height by looking it up in the index and returning it.
    pub fn read_block_header_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut tx = self.storage.env.begin_ro_txn()?;
        let indices = self.storage.indices.read()?;

        indices
            .block_height_index
            .get(&height)
            .and_then(|block_hash| {
                self.storage
                    .get_single_block_header(&mut tx, block_hash)
                    .transpose()
            })
            .transpose()
    }

    /// Retrieves a single block by height by looking it up in the index and returning it.
    fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, FatalStorageError> {
        let mut tx = self.storage.env.begin_ro_txn()?;
        let indices = self.storage.indices.read()?;

        self.storage.get_block_by_height(&mut tx, &indices, height)
    }

    pub fn read_block_header_and_sufficient_finality_signatures_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        self.storage
            .read_block_header_and_sufficient_finality_signatures_by_height(height)
    }

    /// Retrieves the highest block header from the storage, if one exists.
    pub fn read_highest_block_header(&self) -> Result<Option<BlockHeader>, FatalStorageError> {
        let indices = self.storage.indices.read()?;

        let highest_block_hash = match indices.block_height_index.iter().last() {
            Some((_, highest_block_hash)) => highest_block_hash,
            None => return Ok(None),
        };
        self.storage.read_block_header_by_hash(highest_block_hash)
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

/// Purges stale entries from the block body database.
fn initialize_block_body_v1_db(
    env: &Environment,
    block_header_db: &Database,
    block_body_v1_db: &Database,
    deleted_block_body_hashes_raw: &HashSet<&[u8]>,
) -> Result<(), FatalStorageError> {
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

/// Retrieves the block body for the given block header.
/// Returns the block body (if existing) along with the information on whether the block uses the v1
/// or v2 hashing scheme.
fn get_body_for_block_header<Tx: Transaction>(
    tx: &mut Tx,
    block_header: &BlockHeader,
    databases: &Databases,
    verifiable_chunked_hash_activation: EraId,
) -> (Result<Option<BlockBody>, LmdbExtError>, bool) {
    match block_header.hashing_algorithm_version(verifiable_chunked_hash_activation) {
        HashingAlgorithmVersion::V1 => (
            get_single_block_body_v1(tx, block_header.body_hash(), databases.block_body_v1_db),
            true,
        ),
        HashingAlgorithmVersion::V2 => (
            get_single_block_body_v2(tx, block_header.body_hash(), databases),
            false,
        ),
    }
}

fn get_single_block_body_v1<Tx: Transaction>(
    tx: &mut Tx,
    block_body_hash: &Digest,
    block_body_v1_db: Database,
) -> Result<Option<BlockBody>, LmdbExtError> {
    tx.get_value(block_body_v1_db, block_body_hash)
}

/// Retrieves a single Merklized block body in a separate transaction from storage.
fn get_single_block_body_v2<Tx: Transaction>(
    tx: &mut Tx,
    block_body_hash: &Digest,
    Databases {
        block_body_v1_db: _,
        block_body_v2_db,
        deploy_hashes_db,
        transfer_hashes_db,
        proposer_db,
    }: &Databases,
) -> Result<Option<BlockBody>, LmdbExtError> {
    let deploy_hashes_with_proof: MerkleLinkedListNode<Vec<DeployHash>> =
        match get_merkle_linked_list_node(
            tx,
            *block_body_v2_db,
            *deploy_hashes_db,
            block_body_hash,
        )? {
            Some(deploy_hashes_with_proof) => deploy_hashes_with_proof,
            None => return Ok(None),
        };
    let transfer_hashes_with_proof: MerkleLinkedListNode<Vec<DeployHash>> =
        match get_merkle_linked_list_node(
            tx,
            *block_body_v2_db,
            *transfer_hashes_db,
            deploy_hashes_with_proof.merkle_proof_of_rest(),
        )? {
            Some(transfer_hashes_with_proof) => transfer_hashes_with_proof,
            None => return Ok(None),
        };
    let proposer_with_proof: MerkleLinkedListNode<PublicKey> = match get_merkle_linked_list_node(
        tx,
        *block_body_v2_db,
        *proposer_db,
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

// TODO: remove once we run the garbage collection again
#[allow(dead_code)]
fn garbage_collect_block_body_v2_db(
    txn: &mut RwTransaction,
    block_body_v2_db: &Database,
    deploy_hashes_db: &Database,
    transfer_hashes_db: &Database,
    proposer_db: &Database,
    block_body_hash_to_header_map: &BTreeMap<Digest, BlockHeader>,
    verifiable_chunked_hash_activation: EraId,
) -> Result<(), FatalStorageError> {
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
        if header.hashing_algorithm_version(verifiable_chunked_hash_activation)
            != HashingAlgorithmVersion::V2
        {
            // We're only interested in body hashes for V2 blocks here
            continue;
        }
        let mut current_digest = *body_hash;
        let mut live_digests_index = 1;
        while current_digest != Digest::SENTINEL_RFOLD && !live_digests[0].contains(&current_digest)
        {
            live_digests[0].insert(current_digest);
            let (key_to_part_db, merkle_proof_of_rest): (Digest, Digest) =
                match txn.get_value_bytesrepr(*block_body_v2_db, &current_digest)? {
                    Some(slice) => slice,
                    None => {
                        return Err(FatalStorageError::CouldNotFindBlockBodyPart {
                            block_hash: header.hash(verifiable_chunked_hash_activation),
                            merkle_linked_list_node_hash: current_digest,
                        })
                    }
                };
            if live_digests_index < live_digests.len() {
                live_digests[live_digests_index].insert(key_to_part_db);
            } else {
                return Err(FatalStorageError::UnexpectedBlockBodyPart {
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

/// Purges stale entries from the block metadata database.
fn initialize_block_metadata_db(
    env: &Environment,
    block_metadata_db: &Database,
    deleted_block_hashes: &HashSet<&[u8]>,
) -> Result<(), FatalStorageError> {
    info!("initializing block metadata database");
    let mut txn = env.begin_rw_txn()?;
    let mut cursor = txn.open_rw_cursor(*block_metadata_db)?;

    for (raw_key, _) in cursor.iter() {
        if deleted_block_hashes.contains(raw_key) {
            cursor.del(WriteFlags::empty())?;
            continue;
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
    deleted_deploy_hashes: &HashSet<DeployHash>,
) -> Result<(), LmdbExtError> {
    info!("initializing deploy metadata database");

    let mut txn = env.begin_rw_txn()?;
    deleted_deploy_hashes.iter().for_each(|deleted_deploy_hash| {
        if txn.del(*deploy_metadata_db, deleted_deploy_hash, None).is_err() {
            debug!(%deleted_deploy_hash, "not purging from 'deploy_metadata_db' because not existing");
        }
    });
    txn.commit()?;

    info!("deploy metadata database initialized");
    Ok(())
}
