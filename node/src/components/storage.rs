//! Central storage component.
//!
//! The central storage component is in charge of persisting data to disk. Its core functionalities
//! are
//!
//! * storing and loading blocks,
//! * storing and loading deploys,
//! * [temporary until refactored] holding `DeployExecutionInfo` for each deploy,
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
//! ## Errors
//!
//! The storage component itself is panic free and in general reports three classes of errors:
//! Corruption, temporary resource exhaustion and potential bugs.

mod config;
pub(crate) mod disjoint_sequences;
mod error;
mod event;
mod metrics;
mod object_pool;
#[cfg(test)]
mod tests;
mod utils;

use casper_storage::block_store::{
    lmdb::{EraExecutionGap, LmdbBlockStore, SyncLeapLookup},
    types::{
        ApprovalsHashes, BlockExecutionResults, BlockHashHeightAndEra, StateStore, StateStoreKey,
    },
};

use std::{
    borrow::{Borrow, Cow},
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::TryInto,
    fmt::{self, Display, Formatter},
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(test)]
use casper_types::BlockWithSignatures;
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    execution::{ExecutionResult, ExecutionResultV1},
    Approval, ApprovalsHash, AvailableBlockRange, Block, BlockHash, BlockHeader, BlockSignatures,
    BlockSignaturesV1, BlockSignaturesV2, ChainNameDigest, DeployHash, EraId, EvmConfig,
    ExecutionInfo, FinalitySignature, ProtocolVersion, Transaction, TransactionConfig,
    TransactionHash, TransactionId, U512,
};
use datasize::DataSize;
use num_rational::Ratio;
use prometheus::Registry;
use smallvec::SmallVec;
use tracing::{debug, error, info, warn};

use crate::{
    components::{
        fetcher::{FetchItem, FetchResponse},
        Component, ComponentState, InitializedComponent,
    },
    effect::{
        announcements::FatalAnnouncement,
        incoming::{NetRequest, NetRequestIncoming},
        requests::{MarkBlockCompletedRequest, NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    types::{
        BatchedTransaction, BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId,
        BlockWithMetadata, ExecutableBlock, LegacyDeploy, MaxTtl, NodeId, NodeRng, SyncLeap,
        SyncLeapIdentifier, TransactionHeader, VariantMismatch,
    },
    utils::{display_error, WithDir},
};

pub use config::Config;
use disjoint_sequences::{DisjointSequences, Sequence};
pub use error::FatalStorageError;
use error::GetRequestError;
pub(crate) use event::Event;
use metrics::Metrics;
use object_pool::ObjectPool;

const COMPONENT_NAME: &str = "storage";

/// Key under which completed blocks are to be stored.
const COMPLETED_BLOCKS_STORAGE_KEY: &[u8] = b"completed_blocks_disjoint_sequences";
/// Name of the file created when initializing a force resync.
const FORCE_RESYNC_FILE_NAME: &str = "force_resync";

const STORAGE_FILES: [&str; 5] = [
    "data.lmdb",
    "data.lmdb-lock",
    "storage.lmdb",
    "storage.lmdb-lock",
    "sse_index",
];

/// The storage component.
#[derive(DataSize, Debug)]
pub struct Storage {
    /// Storage location.
    root: PathBuf,
    /// Block store.
    block_store: LmdbBlockStore,
    /// Runs of completed blocks known in storage.
    completed_blocks: DisjointSequences,
    /// The activation point era of the current protocol version.
    activation_era: EraId,
    /// The height of the final switch block of the previous protocol version.
    key_block_height_for_activation_point: Option<u64>,
    /// Whether or not memory deduplication is enabled.
    enable_mem_deduplication: bool,
    /// An in-memory pool of already loaded serialized items.
    ///
    /// Keyed by serialized item ID, contains the serialized item.
    serialized_item_pool: ObjectPool<Box<[u8]>>,
    /// The number of eras relative to the highest block's era which are considered as recent for
    /// the purpose of deciding how to respond to a `NetRequest::SyncLeap`.
    recent_era_count: u64,
    #[data_size(skip)]
    metrics: Option<Metrics>,
    /// The maximum TTL of a deploy.
    max_ttl: MaxTtl,
    /// The hash of the chain name.
    chain_name_hash: ChainNameDigest,
    /// The transaction config as specified by the chainspec.
    transaction_config: TransactionConfig,
    /// The EVM config as specified by the chainspec.
    evm_config: EvmConfig,
    /// The utilization of blocks.
    utilization_tracker: BTreeMap<EraId, BTreeMap<u64, u64>>,
    /// Component initialization state.
    state: ComponentState,
    /// The protocol version this node is running.
    #[data_size(skip)]
    protocol_version: ProtocolVersion,
    /// Whether a force resync was requested.
    force_resync: bool,
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum HighestOrphanedBlockResult {
    MissingHighestSequence,
    Orphan(BlockHeader),
    MissingHeader(u64),
}

impl Display for HighestOrphanedBlockResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            HighestOrphanedBlockResult::MissingHighestSequence => {
                write!(f, "missing highest sequence")
            }
            HighestOrphanedBlockResult::Orphan(block_header) => write!(
                f,
                "orphan, height={}, hash={}",
                block_header.height(),
                block_header.block_hash()
            ),
            HighestOrphanedBlockResult::MissingHeader(height) => {
                write!(f, "missing header for block at height: {}", height)
            }
        }
    }
}

impl<REv> Component<REv> for Storage
where
    REv: From<FatalAnnouncement> + From<NetworkRequest<Message>> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.state {
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentState::Uninitialized => {
                warn!(?event, "uninitialized component received event");
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => match self.do_initialize() {
                    Ok(()) => {
                        <Self as InitializedComponent<REv>>::set_state(
                            self,
                            ComponentState::Initialized,
                        );
                        Effects::new()
                    }
                    Err(err) => fatal!(effect_builder, "storage error: {}", err).ignore(),
                },
                _ => {
                    warn!(
                        ?event,
                        "initializing component received non-Initialize event"
                    );
                    Effects::new()
                }
            },
            ComponentState::Initialized => {
                let result = match event {
                    Event::Initialize => {
                        info!("Storage: skipping initialization, already initialized");
                        Ok(Effects::new())
                    }
                    Event::StorageRequest(req) => self.handle_storage_request(*req),
                    Event::NetRequestIncoming(ref incoming) => {
                        match self.handle_net_request_incoming::<REv>(effect_builder, incoming) {
                            Ok(effects) => Ok(effects),
                            Err(GetRequestError::Fatal(fatal_error)) => Err(fatal_error),
                            Err(ref other_err) => {
                                warn!(
                                    sender=%incoming.sender,
                                    err=display_error(other_err),
                                    "error handling net request"
                                );
                                // We could still send the requester a "not found" message, and
                                // could do so even in the fatal case, but it is safer to not do
                                // so at the moment, giving less surface area for possible
                                // amplification attacks.
                                Ok(Effects::new())
                            }
                        }
                    }
                    Event::MarkBlockCompletedRequest(req) => {
                        self.handle_mark_block_completed_request(req)
                    }
                    Event::MakeBlockExecutableRequest(req) => {
                        let ret = self.make_executable_block(&req.block_hash);
                        match ret {
                            Ok(maybe) => Ok(req.responder.respond(maybe).ignore()),
                            Err(err) => Err(err),
                        }
                    }
                };

                // Any error is turned into a fatal effect, the component itself does not panic.
                // Note that we are dropping a lot of responders this way, but since we are
                // crashing with fatal anyway, it should not matter.
                match result {
                    Ok(effects) => effects,
                    Err(err) => fatal!(effect_builder, "storage error: {}", err).ignore(),
                }
            }
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<REv> InitializedComponent<REv> for Storage
where
    REv: From<FatalAnnouncement> + From<NetworkRequest<Message>> + Send,
{
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<REv>>::name(self),
            "component state changed"
        );
        self.state = new_state;
    }
}

/// Opens (and, if necessary, builds the disk-backed indexes of) a node's block store.
pub fn open_block_store(
    cfg: &WithDir<Config>,
    network_name: &str,
) -> Result<(PathBuf, LmdbBlockStore), FatalStorageError> {
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

    let mut block_store = LmdbBlockStore::new(root.as_path(), total_size)?;
    block_store.init()?;

    Ok((root, block_store))
}

impl Storage {
    /// Ctor and init
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: &WithDir<Config>,
        root: PathBuf,
        block_store: LmdbBlockStore,
        protocol_version: ProtocolVersion,
        activation_era: EraId,
        network_name: &str,
        max_ttl: MaxTtl,
        recent_era_count: u64,
        registry: Option<&Registry>,
        force_resync: bool,
        transaction_config: TransactionConfig,
        evm_config: EvmConfig,
    ) -> Result<Self, FatalStorageError> {
        let config = cfg.value();
        let metrics = registry.map(Metrics::new).transpose()?;

        Ok(Self {
            root,
            block_store,
            completed_blocks: Default::default(),
            activation_era,
            key_block_height_for_activation_point: None,
            enable_mem_deduplication: config.enable_mem_deduplication,
            serialized_item_pool: ObjectPool::new(config.mem_pool_prune_interval),
            recent_era_count,
            max_ttl,
            utilization_tracker: BTreeMap::new(),
            metrics,
            chain_name_hash: ChainNameDigest::from_chain_name(network_name),
            transaction_config,
            evm_config,
            state: ComponentState::Uninitialized,
            protocol_version,
            force_resync,
        })
    }

    /// Performs completed-blocks bookkeeping (and force-resync marker handling, if configured).
    fn do_initialize(&mut self) -> Result<(), FatalStorageError> {
        if self.force_resync {
            let force_resync_file_path = self.root_path().join(FORCE_RESYNC_FILE_NAME);
            // Check if resync is already in progress. Force resync will kick
            // in only when the marker file didn't exist before.
            // Use `OpenOptions::create_new` to atomically check for the file
            // presence and create it if necessary.
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&force_resync_file_path)
            {
                Ok(_file) => {
                    // When the force resync marker file was not present and
                    // is now created, initialize force resync.
                    info!("initializing force resync");
                    // Default `storage.completed_blocks`.
                    self.completed_blocks = Default::default();
                    self.persist_completed_blocks()?;
                    // Exit the initialization function early.
                    return Ok(());
                }
                Err(io_err) if io_err.kind() == ErrorKind::AlreadyExists => {
                    info!("skipping force resync as marker file exists");
                }
                Err(io_err) => {
                    warn!(
                        "couldn't operate on the force resync marker file at path {}: {}",
                        force_resync_file_path.to_string_lossy(),
                        io_err
                    );
                }
            }
        }

        {
            let (maybe_state_store, complete_height) = self.block_store.read_completion_state(
                StateStoreKey::new(Cow::Borrowed(COMPLETED_BLOCKS_STORAGE_KEY)),
                self.protocol_version,
            )?;
            match maybe_state_store {
                Some(raw) => {
                    let (mut sequences, _) = DisjointSequences::from_vec(raw)
                        .map_err(FatalStorageError::UnexpectedDeserializationFailure)?;

                    // Truncate the sequences in case we removed blocks via a hard reset.
                    if let Some(height) = complete_height {
                        sequences.truncate(height);
                    } else {
                        // No tip left, the database is empty
                        sequences.clear();
                    }

                    self.completed_blocks = sequences;
                }
                None => {
                    // No state so far. We can make the following observations:
                    //
                    // 1. Any block already in storage from versions prior to 1.5 (no fast-sync)
                    // MUST    have the corresponding global state in contract
                    // runtime due to the way sync    worked previously, so with
                    // the potential exception of finality signatures, we    can
                    // consider all these blocks complete. 2. Any block acquired
                    // from that point onwards was subject to the insertion of the
                    //    appropriate announcements (`BlockCompletedAnnouncement`), which would have
                    //    caused the creation of the completed blocks index, thus would not have
                    //    resulted in a `None` value here.
                    //
                    // Note that a previous run of this version which aborted early could have
                    // stored some blocks and/or block-headers without
                    // completing the sync process. Hence, when setting the
                    // `completed_blocks` in this None case, we'll only consider blocks
                    // from a previous protocol version as complete.

                    if let Some(height) = complete_height {
                        self.completed_blocks = DisjointSequences::new(Sequence::new(0, height));
                    }
                }
            }
        }
        self.persist_completed_blocks()?;
        self.warm_up_utilization_tracker()?;
        Ok(())
    }

    /// Assume:
    /// * E1 is the newest era that the node is aware of (has complete blocks for it).
    /// * the highest completed_blocks sequence is [h_1, h_2]
    ///
    /// The node might be coming back from a crash or joining while E1 is still running.
    /// So effectively the node will have complete blocks that are relevant to E1 on disk.
    /// Because the node might be a validator in E1 we need to figure out what the
    /// utilization tracking for those stored blocks is.
    /// Without this data we will not be able to produce a switch block at the end of E1.
    /// If any of h_n in [h_1,h_2] is from a previous era (E0) - we will stop warming up for that
    /// previous era - the node should never be interested in reexecuting any other block that was
    /// marked complete. if there is an "older" disjoint sequence [h_1`, h_2`] that also has a
    /// height h_n which is in E1 - we won't warm up on that sequence. We are only interested in the
    /// latest consecutive block range of blocks that are in E1
    fn warm_up_utilization_tracker(&mut self) -> Result<(), FatalStorageError> {
        if let Some(sequence) = self.completed_blocks.highest_sequence() {
            let mut data_to_insert = Vec::new();
            let gap = self.block_store.visit_latest_era_execution_records(
                sequence.low()..=sequence.high(),
                |block, results| {
                    let utilization = if results.is_empty() {
                        0
                    } else {
                        Self::calculate_block_utilization(
                            &self.transaction_config,
                            &self.evm_config,
                            &block,
                            &results.into_iter().collect(),
                        )
                    };
                    data_to_insert.push((block.era_id(), block.height(), utilization));
                },
            )?;
            match gap {
                Some(EraExecutionGap::MissingBlock(height)) => {
                    return Err(FatalStorageError::BlockNotFound(height))
                }
                Some(EraExecutionGap::MissingResults(height)) => {
                    return Err(FatalStorageError::MissingExecutionResults(height))
                }
                None => (),
            }
            for (era, height, utilization) in data_to_insert {
                Self::insert_into_utilization_tracker(
                    &mut self.utilization_tracker,
                    era,
                    height,
                    utilization,
                );
            }
        }
        Ok(())
    }

    /// Returns the path to the storage folder.
    pub(crate) fn root_path(&self) -> &Path {
        &self.root
    }

    fn handle_net_request_incoming<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        incoming: &NetRequestIncoming,
    ) -> Result<Effects<Event>, GetRequestError>
    where
        REv: From<NetworkRequest<Message>> + Send,
    {
        if self.enable_mem_deduplication {
            let unique_id = incoming.message.unique_id();

            if let Some(serialized_item) = self
                .serialized_item_pool
                .get(AsRef::<[u8]>::as_ref(&unique_id))
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

        match *(incoming.message) {
            NetRequest::Transaction(ref serialized_id) => {
                let id = decode_item_id::<Transaction>(serialized_id)?;
                let opt_item = self.get_transaction_by_id(id)?;
                let fetch_response = FetchResponse::from_opt(id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::LegacyDeploy(ref serialized_id) => {
                let id = decode_item_id::<LegacyDeploy>(serialized_id)?;
                let opt_item = self.get_legacy_deploy(id)?;
                let fetch_response = FetchResponse::from_opt(id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::Block(ref serialized_id) => {
                let id = decode_item_id::<Block>(serialized_id)?;
                let opt_item = self
                    .block_store
                    .read_block_by_hash(id)
                    .map_err(FatalStorageError::from)?;
                let fetch_response = FetchResponse::from_opt(id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::BlockHeader(ref serialized_id) => {
                let item_id = decode_item_id::<BlockHeader>(serialized_id)?;
                let opt_item = self
                    .block_store
                    .read_block_header_by_hash(item_id)
                    .map_err(FatalStorageError::from)?;
                let fetch_response = FetchResponse::from_opt(item_id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::FinalitySignature(ref serialized_id) => {
                let id = decode_item_id::<FinalitySignature>(serialized_id)?;
                let opt_item = self
                    .block_store
                    .read_block_signatures_by_hash(*id.block_hash())
                    .map_err(FatalStorageError::from)?
                    .and_then(|block_signatures: BlockSignatures| {
                        block_signatures.finality_signature(id.public_key())
                    });

                if let Some(item) = opt_item.as_ref() {
                    if item.block_hash() != id.block_hash() || item.era_id() != id.era_id() {
                        return Err(GetRequestError::FinalitySignatureIdMismatch {
                            requested_id: id,
                            finality_signature: Box::new(item.clone()),
                        });
                    }
                }
                let fetch_response = FetchResponse::from_opt(id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::SyncLeap(ref serialized_id) => {
                let item_id = decode_item_id::<SyncLeap>(serialized_id)?;
                let fetch_response = self.get_sync_leap(item_id)?;

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::ApprovalsHashes(ref serialized_id) => {
                let item_id = decode_item_id::<ApprovalsHashes>(serialized_id)?;
                let opt_item = self
                    .block_store
                    .read_approvals_hashes_by_hash(item_id)
                    .map_err(FatalStorageError::from)?;
                let fetch_response = FetchResponse::from_opt(item_id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::BlockExecutionResults(ref serialized_id) => {
                let item_id = decode_item_id::<BlockExecutionResultsOrChunk>(serialized_id)?;
                let opt_item = self.read_block_execution_results_or_chunk(&item_id)?;
                let fetch_response = FetchResponse::from_opt(item_id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::BatchedTransaction(ref serialized_id) => {
                let id = decode_item_id::<BatchedTransaction>(serialized_id)?;
                let opt_item = self.get_transaction_by_id(id)?.map(BatchedTransaction::new);
                let fetch_response = FetchResponse::from_opt(id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
        }
    }

    /// Handles a storage request.
    fn handle_storage_request(
        &mut self,
        req: StorageRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
        // Note: Database IO is handled in a blocking fashion on purpose throughout this function.
        // The rationale is that long IO operations are very rare and cache misses frequent, so on
        // average the actual execution time will be very low.
        Ok(match req {
            StorageRequest::PutBlock { block, responder } => {
                self.block_store.put_block(&block)?;
                responder.respond(true).ignore()
            }
            StorageRequest::PutApprovalsHashes {
                approvals_hashes,
                responder,
            } => {
                self.block_store.put_approvals_hashes(&approvals_hashes)?;
                responder.respond(true).ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => {
                let maybe_block = self.block_store.read_block_by_hash(block_hash)?;
                responder.respond(maybe_block).ignore()
            }
            StorageRequest::IsBlockStored {
                block_hash,
                responder,
            } => responder
                .respond(self.block_store.block_exists_by_hash(block_hash)?)
                .ignore(),
            StorageRequest::GetApprovalsHashes {
                block_hash,
                responder,
            } => {
                let maybe_item = self.block_store.read_approvals_hashes_by_hash(block_hash)?;
                responder.respond(maybe_item).ignore()
            }
            StorageRequest::GetHighestCompleteBlock { responder } => responder
                .respond(self.get_highest_complete_block()?)
                .ignore(),
            StorageRequest::GetHighestCompleteBlockHeader { responder } => responder
                .respond(self.get_highest_complete_block_header()?)
                .ignore(),
            StorageRequest::GetTransactionsEraIds {
                transaction_hashes,
                responder,
            } => responder
                .respond(
                    self.block_store
                        .read_transaction_era_ids(transaction_hashes)?,
                )
                .ignore(),
            StorageRequest::GetBlockHeader {
                block_hash,
                only_from_available_block_range,
                responder,
            } => {
                let header = self
                    .block_store
                    .read_block_header_by_hash(block_hash)?
                    .filter(|header| {
                        self.should_return_block(header.height(), only_from_available_block_range)
                    });
                responder.respond(header).ignore()
            }
            StorageRequest::GetBlockTransfers {
                block_hash,
                responder,
            } => responder
                .respond(
                    self.block_store
                        .get_or_rebuild_block_transfers(block_hash)?,
                )
                .ignore(),
            StorageRequest::PutTransaction {
                transaction,
                responder,
            } => responder
                .respond(self.block_store.put_transaction(&transaction)?)
                .ignore(),
            StorageRequest::GetTransactions {
                transaction_hashes,
                responder,
            } => responder
                .respond(self.get_transactions_with_finalized_approvals(transaction_hashes.iter())?)
                .ignore(),
            StorageRequest::GetLegacyDeploy {
                deploy_hash,
                responder,
            } => {
                let maybe_legacy_deploy = self.get_legacy_deploy(deploy_hash)?;
                responder.respond(maybe_legacy_deploy).ignore()
            }
            StorageRequest::GetTransaction {
                transaction_id,
                responder,
            } => responder
                .respond(
                    self.block_store
                        .read_transaction_matching_id(transaction_id)?,
                )
                .ignore(),
            StorageRequest::GetTransactionsById {
                transaction_ids,
                responder,
            } => responder
                .respond(
                    self.block_store
                        .read_transactions_matching_ids(transaction_ids)?,
                )
                .ignore(),
            StorageRequest::GetTransactionAndExecutionInfo {
                transaction_hash,
                with_finalized_approvals,
                responder,
            } => responder
                .respond(self.block_store.read_transaction_and_execution_info(
                    transaction_hash,
                    with_finalized_approvals,
                )?)
                .ignore(),
            StorageRequest::IsTransactionStored {
                transaction_id,
                responder,
            } => {
                let has_transaction = self
                    .block_store
                    .transaction_exists(transaction_id.transaction_hash())?;
                responder.respond(has_transaction).ignore()
            }
            StorageRequest::GetExecutionResults {
                block_hash,
                responder,
            } => responder
                .respond(self.get_execution_results_with_transaction_headers(block_hash)?)
                .ignore(),
            StorageRequest::GetBlockExecutionResultsOrChunk { id, responder } => responder
                .respond(self.read_block_execution_results_or_chunk(&id)?)
                .ignore(),
            StorageRequest::PutExecutionResults {
                block_hash,
                block_height,
                era_id,
                execution_results,
                responder,
            } => {
                let results = BlockExecutionResults {
                    block_info: BlockHashHeightAndEra::new(*block_hash, block_height, era_id),
                    exec_results: execution_results,
                };
                if let Some(block) = self.block_store.put_execution_results(&results)? {
                    let utilization = Self::calculate_block_utilization(
                        &self.transaction_config,
                        &self.evm_config,
                        &block,
                        &results.exec_results,
                    );
                    Self::insert_into_utilization_tracker(
                        &mut self.utilization_tracker,
                        era_id,
                        block_height,
                        utilization,
                    );
                }
                responder.respond(()).ignore()
            }
            StorageRequest::GetFinalitySignature { id, responder } => {
                let maybe_sig = self
                    .block_store
                    .read_block_signatures_by_hash(*id.block_hash())?
                    .and_then(|sigs: BlockSignatures| sigs.finality_signature(id.public_key()))
                    .filter(|sig| sig.era_id() == id.era_id());
                responder.respond(maybe_sig).ignore()
            }
            StorageRequest::IsFinalitySignatureStored { id, responder } => {
                let has_signature = self
                    .block_store
                    .read_block_signatures_by_hash(*id.block_hash())?
                    .map(|sigs: BlockSignatures| sigs.has_finality_signature(id.public_key()))
                    .unwrap_or(false);
                responder.respond(has_signature).ignore()
            }
            StorageRequest::GetBlockAndMetadataByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            } => {
                if !self.should_return_block(block_height, only_from_available_block_range) {
                    return Ok(responder.respond(None).ignore());
                }
                let metadata = self
                    .block_store
                    .read_block_and_signatures_at_height(Some(block_height))?
                    .map(|(block, signatures)| {
                        let block_signatures =
                            signatures.unwrap_or_else(|| self.get_default_block_signatures(&block));
                        BlockWithMetadata {
                            block,
                            block_signatures,
                        }
                    });
                responder.respond(metadata).ignore()
            }
            StorageRequest::PutBlockSignatures {
                signatures,
                responder,
            } => responder
                .respond(self.block_store.put_block_signatures(signatures)?)
                .ignore(),
            StorageRequest::PutFinalitySignature {
                signature,
                responder,
            } => responder
                .respond(self.block_store.put_finality_signature(&signature)?)
                .ignore(),
            StorageRequest::GetBlockSignature {
                block_hash,
                public_key,
                responder,
            } => {
                let maybe_signatures =
                    self.block_store.read_block_signatures_by_hash(block_hash)?;
                responder
                    .respond(
                        maybe_signatures
                            .and_then(|signatures| signatures.finality_signature(&public_key)),
                    )
                    .ignore()
            }
            StorageRequest::GetBlockHeaderByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            } => {
                let maybe_header = self
                    .read_block_header_by_height(block_height, only_from_available_block_range)?;
                responder.respond(maybe_header).ignore()
            }
            StorageRequest::GetLatestSwitchBlockHeader { responder } => {
                let maybe_header = self.block_store.read_latest_switch_block_header()?;
                responder.respond(maybe_header).ignore()
            }
            StorageRequest::GetSwitchBlockHeaderByEra { era_id, responder } => {
                let maybe_header = self.block_store.read_block_header_by_era(era_id)?;
                responder.respond(maybe_header).ignore()
            }
            StorageRequest::PutBlockHeader {
                block_header,
                responder,
            } => {
                self.block_store.put_block_header(&block_header)?;
                responder.respond(true).ignore()
            }
            StorageRequest::GetAvailableBlockRange { responder } => {
                responder.respond(self.get_available_block_range()).ignore()
            }
            StorageRequest::StoreFinalizedApprovals {
                ref transaction_hash,
                ref finalized_approvals,
                responder,
            } => {
                info!(txt=?transaction_hash, count=finalized_approvals.len(), "storing finalized approvals {:?}", finalized_approvals);
                responder
                    .respond(self.store_finalized_approvals(transaction_hash, finalized_approvals)?)
                    .ignore()
            }
            StorageRequest::PutExecutedBlock {
                block,
                approvals_hashes,
                execution_results,
                responder,
            } => {
                let block: Block = (*block).clone().into();
                responder
                    .respond(self.put_executed_block(
                        &block,
                        &approvals_hashes,
                        execution_results,
                    )?)
                    .ignore()
            }
            StorageRequest::GetKeyBlockHeightForActivationPoint { responder } => {
                // If we haven't already cached the height, try to retrieve the key block header.
                if self.key_block_height_for_activation_point.is_none() {
                    let key_block_era = self.activation_era.predecessor().unwrap_or_default();
                    let key_block_header =
                        match self.block_store.read_block_header_by_era(key_block_era)? {
                            Some(block_header) => block_header,
                            None => return Ok(responder.respond(None).ignore()),
                        };
                    self.key_block_height_for_activation_point = Some(key_block_header.height());
                }
                responder
                    .respond(self.key_block_height_for_activation_point)
                    .ignore()
            }
            StorageRequest::GetRawData {
                key,
                responder,
                record_id,
            } => {
                let db_table_id = utils::db_table_id_from_record_id(record_id)
                    .map_err(|_| FatalStorageError::UnexpectedRecordId(record_id))?;
                let maybe_data = self.block_store.read_raw(db_table_id, key)?;
                match maybe_data {
                    None => responder.respond(None).ignore(),
                    Some(db_raw) => responder.respond(Some(db_raw)).ignore(),
                }
            }
            StorageRequest::GetEraUtilizationScore {
                era_id,
                block_height,
                switch_block_utilization,
                responder,
            } => {
                let utilization =
                    self.get_era_utilization_score(era_id, block_height, switch_block_utilization);

                responder.respond(utilization).ignore()
            }
        })
    }

    pub(crate) fn read_block_header_by_height(
        &self,
        block_height: u64,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        if !(self.should_return_block(block_height, only_from_available_block_range)) {
            Ok(None)
        } else {
            self.block_store
                .read_block_header_at_height(block_height)
                .map_err(FatalStorageError::from)
        }
    }

    pub(crate) fn get_switch_block_by_era_id(
        &self,
        era_id: &EraId,
    ) -> Result<Option<Block>, FatalStorageError> {
        self.block_store
            .read_switch_block_by_era(*era_id)
            .map_err(FatalStorageError::from)
    }

    /// Retrieves a set of transactions, along with their potential finalized approvals.
    #[allow(clippy::type_complexity)]
    fn get_transactions_with_finalized_approvals<'a>(
        &self,
        transaction_hashes: impl Iterator<Item = &'a TransactionHash>,
    ) -> Result<SmallVec<[Option<(Transaction, Option<BTreeSet<Approval>>)>; 1]>, FatalStorageError>
    {
        Ok(self
            .block_store
            .read_transactions_with_finalized_approvals(transaction_hashes.copied())?
            .into_iter()
            .collect())
    }

    pub(crate) fn put_executed_block(
        &mut self,
        block: &Block,
        approvals_hashes: &ApprovalsHashes,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, FatalStorageError> {
        let era_id = block.era_id();
        let utilization = Self::calculate_block_utilization(
            &self.transaction_config,
            &self.evm_config,
            block,
            &execution_results,
        );

        self.block_store
            .put_executed_block(block, approvals_hashes, execution_results)?;

        debug!("Utilization for block is {utilization}");

        Self::insert_into_utilization_tracker(
            &mut self.utilization_tracker,
            era_id,
            block.height(),
            utilization,
        );

        Ok(true)
    }

    /// Handles a [`BlockCompletedAnnouncement`].
    fn handle_mark_block_completed_request(
        &mut self,
        MarkBlockCompletedRequest {
            block_height,
            responder,
        }: MarkBlockCompletedRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
        let is_new = self.mark_block_complete(block_height)?;
        Ok(responder.respond(is_new).ignore())
    }

    /// Marks the block at height `block_height` as complete by inserting it
    /// into the `completed_blocks` index and storing it to disk.
    fn mark_block_complete(&mut self, block_height: u64) -> Result<bool, FatalStorageError> {
        let is_new = self.completed_blocks.insert(block_height);
        if is_new {
            self.persist_completed_blocks()?;
            info!(
                "Storage: marked block {} complete: {}",
                block_height,
                self.get_available_block_range()
            );
            self.update_chain_height_metrics();
        } else {
            debug!(
                "Storage: tried to mark already-complete block {} complete",
                block_height
            );
        }
        Ok(is_new)
    }

    /// Persists the completed blocks disjoint sequences state to the database.
    fn persist_completed_blocks(&mut self) -> Result<(), FatalStorageError> {
        let serialized = self
            .completed_blocks
            .to_bytes()
            .map_err(FatalStorageError::UnexpectedSerializationFailure)?;
        self.block_store.put_state_store(&StateStore {
            key: Cow::Borrowed(COMPLETED_BLOCKS_STORAGE_KEY),
            value: serialized,
        })?;
        Ok(())
    }

    /// Retrieves the height of the highest complete block (if any).
    pub(crate) fn highest_complete_block_height(&self) -> Option<u64> {
        self.completed_blocks.highest_sequence().map(Sequence::high)
    }

    /// Retrieves the contiguous segment of the block chain starting at the highest known switch
    /// block such that the blocks' timestamps cover a duration of at least the max TTL for deploys
    /// (a chainspec setting).
    ///
    /// If storage doesn't hold enough blocks to cover the specified duration, it will still return
    /// the highest contiguous segment starting at the highest switch block which it does hold.
    pub(crate) fn read_blocks_for_replay_protection(
        &self,
    ) -> Result<Vec<Block>, FatalStorageError> {
        let heights = self
            .completed_blocks
            .sequences()
            .iter()
            .rev()
            .flat_map(|sequence| (sequence.low()..=sequence.high()).rev());
        Ok(self
            .block_store
            .read_blocks_for_replay_protection(heights, self.max_ttl.value())?)
    }

    /// Returns an executable block.
    pub(crate) fn make_executable_block(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<ExecutableBlock>, FatalStorageError> {
        let Some((block, transactions, approvals_hashes)) =
            self.block_store.read_block_for_execution(*block_hash)?
        else {
            error!(%block_hash, "unable to make executable block");
            return Ok(None);
        };
        if let Some(approvals_hashes) = approvals_hashes {
            let approvals_hashes = approvals_hashes.approvals_hashes();
            if transactions.len() != approvals_hashes.len() {
                error!(
                    ?block_hash,
                    "Storage: transaction hashes length mismatch {}", block_hash
                );
                return Err(FatalStorageError::ApprovalsHashesLengthMismatch {
                    block_hash: *block_hash,
                    expected: transactions.len(),
                    actual: approvals_hashes.len(),
                });
            }
            for (transaction, hash) in transactions.iter().zip(approvals_hashes) {
                let computed_hash = transaction.compute_approvals_hash().map_err(|error| {
                    error!(%error, "failed to serialize approvals");
                    FatalStorageError::UnexpectedSerializationFailure(error)
                })?;
                if computed_hash == hash {
                    continue;
                }
                // This should be unreachable as the `BlockSynchronizer` should ensure we have the
                // correct approvals before it then calls this method.  By returning `Ok(None)` the
                // node would be stalled at this block, but should eventually sync leap due to lack
                // of progress.  It would then backfill this block without executing it.
                error!(?block_hash, "Storage: transaction with incorrect approvals");
                return Ok(None);
            }
        }

        let executable_block = ExecutableBlock::from_block_and_transactions(block, transactions);
        info!(%block_hash, "Storage: created {}", executable_block);
        Ok(Some(executable_block))
    }

    /// Retrieves the highest complete block header from storage, if one exists. May return an
    /// LMDB error.
    fn get_highest_complete_block_header(&self) -> Result<Option<BlockHeader>, FatalStorageError> {
        let Some(height) = self.highest_complete_block_height() else {
            return Ok(None);
        };

        self.block_store
            .read_block_header_at_height(height)
            .map_err(FatalStorageError::from)
    }

    /// Retrieves the highest complete block from storage, if one exists. May return an LMDB error.
    pub fn get_highest_complete_block(&self) -> Result<Option<Block>, FatalStorageError> {
        let Some(height) = self.highest_complete_block_height() else {
            return Ok(None);
        };

        self.block_store
            .read_block_at_height(height)
            .map_err(FatalStorageError::from)
    }

    /// Stores a set of finalized approvals if they are different to the approvals in the original
    /// transaction and if they are different to existing finalized approvals if any.
    ///
    /// Returns `true` if the provided approvals were stored.
    fn store_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
        finalized_approvals: &BTreeSet<Approval>,
    ) -> Result<bool, FatalStorageError> {
        self.block_store
            .put_finalized_approvals(*transaction_hash, finalized_approvals)?
            .ok_or(FatalStorageError::UnexpectedFinalizedApprovals {
                transaction_hash: *transaction_hash,
            })
    }

    /// Retrieves a deploy from the deploy store by deploy hash.
    fn get_legacy_deploy(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<LegacyDeploy>, FatalStorageError> {
        let transaction_hash = TransactionHash::from(deploy_hash);
        let Some((transaction, approvals)) = self
            .block_store
            .read_transaction_with_finalized_approvals(transaction_hash)?
        else {
            return Ok(None);
        };
        let transaction = match approvals {
            Some(approvals) => transaction.with_approvals(approvals),
            None => transaction,
        };

        match transaction {
            Transaction::Deploy(deploy) => Ok(Some(LegacyDeploy::from(deploy))),
            transaction @ (Transaction::V1(_) | Transaction::Evm(_)) => {
                let mismatch = VariantMismatch(Box::new((transaction_hash, transaction)));
                error!(%mismatch, "failed getting legacy deploy");
                Err(FatalStorageError::from(mismatch))
            }
        }
    }

    /// Retrieves a transaction by transaction ID.
    fn get_transaction_by_id(
        &self,
        transaction_id: TransactionId,
    ) -> Result<Option<Transaction>, FatalStorageError> {
        let Some((transaction, finalized_approvals)) = self
            .block_store
            .read_transaction_for_fetch(transaction_id)?
        else {
            return Ok(None);
        };
        let Some(finalized_approvals) = finalized_approvals else {
            return Ok(Some(transaction));
        };

        let approvals_hash = ApprovalsHash::compute(&finalized_approvals).map_err(|error| {
            error!(%error, "failed to calculate finalized transaction approvals hash");
            FatalStorageError::UnexpectedSerializationFailure(error)
        })?;
        if approvals_hash != transaction_id.approvals_hash()
            || (transaction.as_evm().is_some() && finalized_approvals != transaction.approvals())
        {
            return Ok(None);
        }
        Ok(Some(transaction.with_approvals(finalized_approvals)))
    }

    pub(crate) fn get_sync_leap(
        &self,
        sync_leap_identifier: SyncLeapIdentifier,
    ) -> Result<FetchResponse<SyncLeap, SyncLeapIdentifier>, FatalStorageError> {
        let trusted_ancestor_only = sync_leap_identifier.trusted_ancestor_only();
        let result = self.block_store.read_sync_leap(
            sync_leap_identifier.block_hash(),
            trusted_ancestor_only,
            self.get_available_block_range(),
            self.highest_complete_block_height(),
            self.recent_era_count,
            self.chain_name_hash,
        )?;
        Ok(match result {
            SyncLeapLookup::NotFound => FetchResponse::NotFound(sync_leap_identifier),
            SyncLeapLookup::NotProvided => FetchResponse::NotProvided(sync_leap_identifier),
            SyncLeapLookup::Found {
                trusted_block_header,
                trusted_ancestor_headers,
                block_headers_with_signatures,
            } => FetchResponse::Fetched(SyncLeap {
                trusted_ancestor_only,
                trusted_block_header: *trusted_block_header,
                trusted_ancestor_headers,
                block_headers_with_signatures,
            }),
        })
    }

    /// Creates a serialized representation of a `FetchResponse` and the resulting message.
    ///
    /// If the given item is `Some`, returns a serialization of `FetchResponse::Fetched`. If
    /// enabled, the given serialization is also added to the in-memory pool.
    ///
    /// If the given item is `None`, returns a non-pooled serialization of
    /// `FetchResponse::NotFound`.
    fn update_pool_and_send<REv, T>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        sender: NodeId,
        serialized_id: &[u8],
        fetch_response: FetchResponse<T, T::Id>,
    ) -> Result<Effects<Event>, FatalStorageError>
    where
        REv: From<NetworkRequest<Message>> + Send,
        T: FetchItem,
    {
        let serialized = fetch_response
            .to_serialized()
            .map_err(FatalStorageError::StoredItemSerializationFailure)?;
        let shared: Arc<[u8]> = serialized.into();

        if self.enable_mem_deduplication && fetch_response.was_found() {
            self.serialized_item_pool
                .put(serialized_id.into(), Arc::downgrade(&shared));
        }

        let message = Message::new_get_response_from_serialized(<T as FetchItem>::TAG, shared);
        Ok(effect_builder.send_message(sender, message).ignore())
    }

    /// Returns `true` if the storage should attempt to return a block. Depending on the
    /// `only_from_available_block_range` flag it should be unconditional or restricted by the
    /// available block range.
    fn should_return_block(
        &self,
        block_height: u64,
        only_from_available_block_range: bool,
    ) -> bool {
        if only_from_available_block_range {
            self.get_available_block_range().contains(block_height)
        } else {
            true
        }
    }

    pub(crate) fn get_available_block_range(&self) -> AvailableBlockRange {
        match self.completed_blocks.highest_sequence() {
            Some(&seq) => seq.into(),
            None => AvailableBlockRange::RANGE_0_0,
        }
    }

    pub(crate) fn get_highest_orphaned_block_header(&self) -> HighestOrphanedBlockResult {
        match self.completed_blocks.highest_sequence() {
            None => HighestOrphanedBlockResult::MissingHighestSequence,
            Some(seq) => {
                let low = seq.low();
                match self.block_store.read_block_at_height(low) {
                    Ok(Some(block)) => HighestOrphanedBlockResult::Orphan(block.clone_header()),
                    Ok(None) | Err(_) => HighestOrphanedBlockResult::MissingHeader(low),
                }
            }
        }
    }

    /// Returns `count` highest switch block headers, sorted from lowest (oldest) to highest.
    pub(crate) fn read_highest_switch_block_headers(
        &self,
        count: u64,
    ) -> Result<Vec<BlockHeader>, FatalStorageError> {
        Ok(self.block_store.read_highest_switch_block_headers(count)?)
    }

    fn read_block_execution_results_or_chunk(
        &self,
        request: &BlockExecutionResultsOrChunkId,
    ) -> Result<Option<BlockExecutionResultsOrChunk>, FatalStorageError> {
        let execution_results = match self
            .block_store
            .read_block_execution_results(*request.block_hash())?
        {
            Some(execution_results) => execution_results
                .into_iter()
                .map(|(_deploy_hash, execution_result)| execution_result)
                .collect(),
            None => return Ok(None),
        };
        Ok(BlockExecutionResultsOrChunk::new(
            *request.block_hash(),
            request.chunk_index(),
            execution_results,
        ))
    }

    fn get_default_block_signatures(&self, block: &Block) -> BlockSignatures {
        match block {
            Block::V1(block) => BlockSignaturesV1::new(*block.hash(), block.era_id()).into(),
            Block::V2(block) => BlockSignaturesV2::new(
                *block.hash(),
                block.height(),
                block.era_id(),
                self.chain_name_hash,
            )
            .into(),
        }
    }

    fn update_chain_height_metrics(&self) {
        if let Some(metrics) = self.metrics.as_ref() {
            if let Some(sequence) = self.completed_blocks.highest_sequence() {
                let highest_available_block: i64 = sequence.high().try_into().unwrap_or(i64::MIN);
                let lowest_available_block: i64 = sequence.low().try_into().unwrap_or(i64::MIN);
                metrics.chain_height.set(highest_available_block);
                metrics.highest_available_block.set(highest_available_block);
                metrics.lowest_available_block.set(lowest_available_block);
            }
        }
    }

    pub(crate) fn read_block_header_by_hash(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.block_store
            .read_block_header_by_hash(*block_hash)
            .map_err(FatalStorageError::from)
    }

    #[allow(clippy::type_complexity)]
    fn get_execution_results_with_transaction_headers(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, TransactionHeader, ExecutionResult)>>, FatalStorageError>
    {
        let Some(records) = self
            .block_store
            .read_block_transactions_and_results(block_hash)?
        else {
            return Ok(None);
        };
        let records = records
            .into_iter()
            .map(|(hash, transaction, result)| {
                let header = match transaction {
                    Transaction::Deploy(deploy) => deploy.take_header().into(),
                    Transaction::V1(transaction) => (&transaction).into(),
                    Transaction::Evm(transaction) => transaction.as_ref().into(),
                };
                (hash, header, result)
            })
            .collect();
        Ok(Some(records))
    }

    fn get_era_utilization_score(
        &mut self,
        era_id: EraId,
        era_switch_block_height: u64,
        era_switch_block_utilization: u64,
    ) -> Option<(u64, u64, u64)> {
        let ret = match self.utilization_tracker.get_mut(&era_id) {
            Some(utilization) => {
                utilization
                    .entry(era_switch_block_height)
                    .or_insert(era_switch_block_utilization);

                let era_utilization = utilization.values().sum();
                let block_count = utilization.keys().len() as u64;
                let total_blocks_for_era = match era_id.predecessor() {
                    Some(previous_era) => {
                        let previous_switch_block_height =
                            match self.block_store.get_switch_block_height(previous_era) {
                                Ok(Some(height)) => height,
                                Ok(None) | Err(_) => return None,
                            };
                        // Determine expected number of blocks from the block_height
                        // minus the height of the previous switch block
                        // sw-e1 -> b1 b2 b3 b4 sw-e2
                        // 11       12 13 14 15 16
                        // answer: 5 (16-11)
                        era_switch_block_height.saturating_sub(previous_switch_block_height)
                    }
                    // Genesis case
                    None => era_switch_block_height,
                };

                Some((era_utilization, block_count, total_blocks_for_era))
            }
            None => {
                let mut utilization = BTreeMap::new();
                utilization.insert(era_switch_block_height, era_switch_block_utilization);

                self.utilization_tracker.insert(era_id, utilization);

                let block_count = 1u64;
                let total_blocks_for_era = block_count;
                Some((
                    era_switch_block_utilization,
                    block_count,
                    total_blocks_for_era,
                ))
            }
        };

        self.utilization_tracker
            .retain(|key_era_id, _| key_era_id.value() >= era_id.value());

        ret
    }

    fn calculate_block_utilization(
        transaction_config_input: impl Borrow<TransactionConfig>,
        evm_config_input: impl Borrow<EvmConfig>,
        block: &Block,
        execution_results: &HashMap<TransactionHash, ExecutionResult>,
    ) -> u64 {
        let transaction_config = transaction_config_input.borrow();
        let evm_config = evm_config_input.borrow();
        let block_utilization_score = block.block_utilization(transaction_config, evm_config);
        let has_hit_slot_limit = block.has_hit_slot_capacity(transaction_config, evm_config);

        let utilization = if has_hit_slot_limit {
            debug!("Block is at slot capacity, using slot utilization score");
            block_utilization_score
        } else if execution_results.is_empty() {
            0u64
        } else {
            let total_gas_utilization = {
                let total_gas_limit: U512 = execution_results
                    .values()
                    .map(|results| match results {
                        ExecutionResult::V1(v1_result) => match v1_result {
                            ExecutionResultV1::Failure { cost, .. } => *cost,
                            ExecutionResultV1::Success { cost, .. } => *cost,
                        },
                        ExecutionResult::V2(v2_result) => v2_result.limit.value(),
                        ExecutionResult::Evm(evm_result) => evm_result.limit.value(),
                    })
                    .sum();

                let consumed: u64 = total_gas_limit.as_u64();
                let block_gas_limit = transaction_config.block_gas_limit;

                Ratio::new(consumed * 100u64, block_gas_limit).to_integer()
            };
            debug!("Gas utilization at {total_gas_utilization}");

            let total_size_utilization = {
                let size_used: u64 = execution_results
                    .values()
                    .map(|results| {
                        if let ExecutionResult::V2(result) = results {
                            result.size_estimate
                        } else if let ExecutionResult::Evm(result) = results {
                            result.size_estimate
                        } else {
                            0u64
                        }
                    })
                    .sum();

                let block_size_limit = transaction_config.max_block_size as u64;
                Ratio::new(size_used * 100, block_size_limit).to_integer()
            };

            debug!("Storage utilization at {total_size_utilization}");

            let scores = [
                block_utilization_score,
                total_size_utilization,
                total_gas_utilization,
            ];

            match scores.iter().max() {
                Some(max_utilization) => *max_utilization,
                None => {
                    // This should never happen as we just created the scores vector to find the
                    // max value
                    warn!("Unable to determine max utilization, marking 0 utilization");
                    0u64
                }
            }
        };
        utilization
    }

    fn insert_into_utilization_tracker(
        utilization_tracker: &mut BTreeMap<EraId, BTreeMap<u64, u64>>,
        era_id: EraId,
        block_height: u64,
        utilization: u64,
    ) {
        match utilization_tracker.get_mut(&era_id) {
            Some(block_score) => {
                block_score.insert(block_height, utilization);
            }
            None => {
                let mut block_score = BTreeMap::new();
                block_score.insert(block_height, utilization);
                utilization_tracker.insert(era_id, block_score);
            }
        }
    }
}

/// Decodes an item's ID, typically from an incoming request.
fn decode_item_id<T>(raw: &[u8]) -> Result<T::Id, GetRequestError>
where
    T: FetchItem,
{
    bincode::deserialize(raw).map_err(GetRequestError::MalformedIncomingItemId)
}

fn should_move_storage_files_to_network_subdir(
    root: &Path,
    file_names: &[&str],
) -> Result<bool, FatalStorageError> {
    let mut files_found = vec![];
    let mut files_not_found = vec![];

    for file_name in file_names {
        let file_path = root.join(file_name);

        if file_path.exists() {
            files_found.push(file_path);
        } else {
            files_not_found.push(file_path);
        }
    }

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
    /// Drives this component through its `InitializedComponent` initialization step, mirroring
    /// what the reactor's `initialize_next_component` does at startup. Test harnesses across the
    /// crate that construct a `Storage` directly (rather than via the full reactor) must call
    /// this before issuing any `StorageRequest`s against it.
    pub(crate) fn initialize_for_test(&mut self) {
        self.do_initialize()
            .expect("storage initialization should succeed");
        self.state = ComponentState::Initialized;
    }

    /// Directly returns a transaction with finalized approvals from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn get_transaction_with_finalized_approvals_by_hash(
        &self,
        transaction_hash: &TransactionHash,
    ) -> Option<(Transaction, Option<BTreeSet<Approval>>)> {
        self.block_store
            .read_transaction_with_finalized_approvals(*transaction_hash)
            .expect("could not retrieve a transaction with finalized approvals from storage")
    }

    /// Directly returns an execution result from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn read_execution_result(
        &self,
        transaction_hash: &TransactionHash,
    ) -> Option<ExecutionResult> {
        self.block_store
            .read_execution_result(*transaction_hash)
            .expect("could not retrieve execution result from storage")
    }

    /// Directly returns a transaction from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn get_transaction_by_hash(
        &self,
        transaction_hash: TransactionHash,
    ) -> Option<Transaction> {
        self.block_store
            .read_transaction(transaction_hash)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_block_by_hash(&self, block_hash: BlockHash) -> Option<Block> {
        self.block_store
            .read_block_by_hash(block_hash)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_block_by_height(&self, height: u64) -> Option<Block> {
        self.block_store
            .read_block_at_height(height)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_highest_block(&self) -> Option<Block> {
        self.block_store
            .read_tip_block()
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_highest_block_header(&self) -> Option<BlockHeader> {
        self.block_store
            .read_tip_block_header()
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn get_finality_signatures_for_block(
        &self,
        block_hash: BlockHash,
    ) -> Option<BlockSignatures> {
        self.block_store
            .read_block_signatures_by_hash(block_hash)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_switch_block_by_era_id(&self, era_id: EraId) -> Option<Block> {
        self.block_store
            .read_switch_block_by_era(era_id)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_block_with_signatures_by_hash(
        &self,
        block_hash: BlockHash,
        only_from_available_block_range: bool,
    ) -> Option<BlockWithSignatures> {
        let (block, signatures) = self
            .block_store
            .read_block_and_signatures_by_hash(block_hash)
            .expect("should read block and signatures")?;

        if !(self.should_return_block(block.height(), only_from_available_block_range)) {
            return None;
        }
        if block_hash != *block.hash() {
            error!(
                queried_block_hash = ?block_hash,
                actual_block_hash = ?block.hash(),
                "block not stored under hash"
            );
            debug_assert_eq!(&block_hash, block.hash());
            return None;
        }
        let block_signatures =
            signatures.unwrap_or_else(|| self.get_default_block_signatures(&block));
        if block_signatures.is_verified().is_err() {
            error!(?block, "invalid block signatures for block");
            debug_assert!(block_signatures.is_verified().is_ok());
            return None;
        }
        Some(BlockWithSignatures::new(block, block_signatures))
    }

    pub(crate) fn read_block_with_signatures_by_height(
        &self,
        height: u64,
        only_from_available_block_range: bool,
    ) -> Option<BlockWithSignatures> {
        if !(self.should_return_block(height, only_from_available_block_range)) {
            return None;
        }
        let (block, signatures) = self
            .block_store
            .read_block_and_signatures_at_height(Some(height))
            .expect("should read block and signatures")?;
        let block_signatures =
            signatures.unwrap_or_else(|| self.get_default_block_signatures(&block));
        Some(BlockWithSignatures::new(block, block_signatures))
    }

    pub(crate) fn read_highest_block_with_signatures(
        &self,
        only_from_available_block_range: bool,
    ) -> Option<BlockWithSignatures> {
        let height = if only_from_available_block_range {
            Some(self.highest_complete_block_height()?)
        } else {
            None
        };
        let (block, signatures) = self
            .block_store
            .read_block_and_signatures_at_height(height)
            .expect("should read block and signatures")?;
        let signatures = signatures.unwrap_or_else(|| self.get_default_block_signatures(&block));
        Some(BlockWithSignatures::new(block, signatures))
    }

    pub(crate) fn read_execution_info(
        &self,
        transaction_hash: TransactionHash,
    ) -> Option<ExecutionInfo> {
        self.block_store
            .read_execution_info(transaction_hash)
            .expect("should read execution info")
    }

    pub(crate) fn delete_block_utilization_score_by_block_hash(&mut self, block_hash: BlockHash) {
        let block_header = self
            .block_store
            .read_block_header_by_hash(block_hash)
            .expect("should read")
            .expect("must have header");

        let era = block_header.era_id();
        let height = block_header.height();

        let era_score = self
            .utilization_tracker
            .get_mut(&era)
            .expect("must have era tracker");
        era_score
            .remove(&height)
            .expect("must have previous entry for this height");
    }
}
