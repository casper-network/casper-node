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
    lmdb::{IndexedLmdbBlockStore, LmdbBlockStore},
    types::{
        ApprovalsHashes, BlockExecutionResults, BlockHashHeightAndEra, BlockHeight, BlockTransfers,
        LatestSwitchBlock, StateStore, StateStoreKey, Tip, TransactionFinalizedApprovals,
    },
    BlockStoreError, BlockStoreProvider, BlockStoreTransaction, DataReader, DataWriter,
};

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    convert::TryInto,
    fmt::{self, Display, Formatter},
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use casper_storage::DbRawBytesSpec;
#[cfg(test)]
use casper_types::SignedBlock;
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    execution::{execution_result_v1, ExecutionResult, ExecutionResultV1, ExecutionResultV2},
    Approval, ApprovalsHash, AvailableBlockRange, Block, BlockBody, BlockHash, BlockHeader,
    BlockSignatures, BlockSignaturesV1, BlockSignaturesV2, BlockV2, ChainNameDigest, DeployHash,
    EraId, ExecutionInfo, FinalitySignature, ProtocolVersion, SignedBlockHeader, Timestamp,
    Transaction, TransactionConfig, TransactionHash, TransactionId, Transfer, U512,
};
use datasize::DataSize;
use num_rational::Ratio;
use prometheus::Registry;
use smallvec::SmallVec;
use tracing::{debug, error, info, warn};

use crate::{
    components::{
        fetcher::{FetchItem, FetchResponse},
        Component,
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
        BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId, BlockWithMetadata,
        ExecutableBlock, LegacyDeploy, MaxTtl, NodeId, NodeRng, SyncLeap, SyncLeapIdentifier,
        TransactionHeader, VariantMismatch,
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
    /// Block store
    pub(crate) block_store: IndexedLmdbBlockStore,
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
    /// The utilization of blocks.
    utilization_tracker: BTreeMap<EraId, BTreeMap<u64, u64>>,
}

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
        let result = match event {
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
                        // We could still send the requester a "not found" message, and could do
                        // so even in the fatal case, but it is safer to not do so at the
                        // moment, giving less surface area for possible amplification attacks.
                        Ok(Effects::new())
                    }
                }
            }
            Event::MarkBlockCompletedRequest(req) => self.handle_mark_block_completed_request(req),
            Event::MakeBlockExecutableRequest(req) => {
                let ret = self.make_executable_block(&req.block_hash);
                match ret {
                    Ok(maybe) => Ok(req.responder.respond(maybe).ignore()),
                    Err(err) => Err(err),
                }
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

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl Storage {
    /// Creates a new storage component.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: &WithDir<Config>,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
        activation_era: EraId,
        network_name: &str,
        max_ttl: MaxTtl,
        recent_era_count: u64,
        registry: Option<&Registry>,
        force_resync: bool,
        transaction_config: TransactionConfig,
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

        let block_store = LmdbBlockStore::new(root.as_path(), total_size)?;
        let indexed_block_store =
            IndexedLmdbBlockStore::new(block_store, hard_reset_to_start_of_era, protocol_version)?;

        let metrics = registry.map(Metrics::new).transpose()?;

        let mut component = Self {
            root,
            block_store: indexed_block_store,
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
        };

        if force_resync {
            let force_resync_file_path = component.root_path().join(FORCE_RESYNC_FILE_NAME);
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
                    component.completed_blocks = Default::default();
                    component.persist_completed_blocks()?;
                    // Exit the initialization function early.
                    return Ok(component);
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
            let ro_txn = component.block_store.checkout_ro()?;
            let maybe_state_store: Option<Vec<u8>> = ro_txn.read(StateStoreKey::new(
                Cow::Borrowed(COMPLETED_BLOCKS_STORAGE_KEY),
            ))?;
            match maybe_state_store {
                Some(raw) => {
                    let (mut sequences, _) = DisjointSequences::from_vec(raw)
                        .map_err(FatalStorageError::UnexpectedDeserializationFailure)?;

                    // Truncate the sequences in case we removed blocks via a hard reset.
                    if let Some(header) = DataReader::<Tip, BlockHeader>::read(&ro_txn, Tip)? {
                        sequences.truncate(header.height());
                    }

                    component.completed_blocks = sequences;
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

                    let maybe_block_header: Option<BlockHeader> = ro_txn.read(Tip)?;
                    if let Some(highest_block_header) = maybe_block_header {
                        for height in (0..=highest_block_header.height()).rev() {
                            let maybe_header: Option<BlockHeader> = ro_txn.read(height)?;
                            match maybe_header {
                                Some(header) if header.protocol_version() < protocol_version => {
                                    component.completed_blocks =
                                        DisjointSequences::new(Sequence::new(0, header.height()));
                                    break;
                                }
                                _ => {}
                            }
                        }
                    };
                }
            }
        }
        component.persist_completed_blocks()?;
        Ok(component)
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
                let opt_item: Option<Block> = self
                    .block_store
                    .checkout_ro()
                    .map_err(FatalStorageError::from)?
                    .read(id)
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
                let opt_item: Option<BlockHeader> = self
                    .block_store
                    .checkout_ro()
                    .map_err(FatalStorageError::from)?
                    .read(item_id)
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
                    .checkout_ro()
                    .map_err(FatalStorageError::from)?
                    .read(*id.block_hash())
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
                let opt_item: Option<ApprovalsHashes> = self
                    .block_store
                    .checkout_ro()
                    .map_err(FatalStorageError::from)?
                    .read(item_id)
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
                let mut rw_txn = self.block_store.checkout_rw()?;
                let _ = rw_txn.write(&*block)?;
                rw_txn.commit()?;
                responder.respond(true).ignore()
            }
            StorageRequest::PutApprovalsHashes {
                approvals_hashes,
                responder,
            } => {
                let mut rw_txn = self.block_store.checkout_rw()?;
                let _ = rw_txn.write(&*approvals_hashes)?;
                rw_txn.commit()?;
                responder.respond(true).ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => {
                let maybe_block = self.block_store.checkout_ro()?.read(block_hash)?;
                responder.respond(maybe_block).ignore()
            }
            StorageRequest::IsBlockStored {
                block_hash,
                responder,
            } => {
                let txn = self.block_store.checkout_ro()?;
                responder
                    .respond(DataReader::<BlockHash, Block>::exists(&txn, block_hash)?)
                    .ignore()
            }
            StorageRequest::GetApprovalsHashes {
                block_hash,
                responder,
            } => responder
                .respond(self.block_store.checkout_ro()?.read(block_hash)?)
                .ignore(),
            StorageRequest::GetHighestCompleteBlock { responder } => responder
                .respond(self.get_highest_complete_block()?)
                .ignore(),
            StorageRequest::GetHighestCompleteBlockHeader { responder } => responder
                .respond(self.get_highest_complete_block_header()?)
                .ignore(),
            StorageRequest::GetTransactionsEraIds {
                transaction_hashes,
                responder,
            } => {
                let mut era_ids = HashSet::new();
                let txn = self.block_store.checkout_ro()?;
                for transaction_hash in &transaction_hashes {
                    let maybe_block_info: Option<BlockHashHeightAndEra> =
                        txn.read(*transaction_hash)?;
                    if let Some(block_info) = maybe_block_info {
                        era_ids.insert(block_info.era_id);
                    }
                }
                responder.respond(era_ids).ignore()
            }
            StorageRequest::GetBlockHeader {
                block_hash,
                only_from_available_block_range,
                responder,
            } => {
                let txn = self.block_store.checkout_ro()?;
                responder
                    .respond(self.get_single_block_header_restricted(
                        &txn,
                        &block_hash,
                        only_from_available_block_range,
                    )?)
                    .ignore()
            }
            StorageRequest::GetBlockTransfers {
                block_hash,
                responder,
            } => {
                let maybe_transfers = self.get_transfers(&block_hash)?;
                responder.respond(maybe_transfers).ignore()
            }
            StorageRequest::PutTransaction {
                transaction,
                responder,
            } => {
                let mut rw_txn = self.block_store.checkout_rw()?;
                if DataReader::<TransactionHash, Transaction>::exists(&rw_txn, transaction.hash())?
                {
                    responder.respond(false).ignore()
                } else {
                    let _ = rw_txn.write(&*transaction)?;
                    rw_txn.commit()?;
                    responder.respond(true).ignore()
                }
            }
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
            } => {
                let ro_txn = self.block_store.checkout_ro()?;
                let maybe_transaction = match Self::get_transaction_with_finalized_approvals(
                    &ro_txn,
                    &transaction_id.transaction_hash(),
                )? {
                    None => None,
                    Some((transaction, maybe_approvals)) => {
                        let transaction = if let Some(approvals) = maybe_approvals {
                            transaction.with_approvals(approvals)
                        } else {
                            transaction
                        };
                        (transaction.fetch_id() == transaction_id).then_some(transaction)
                    }
                };
                responder.respond(maybe_transaction).ignore()
            }
            StorageRequest::GetTransactionAndExecutionInfo {
                transaction_hash,
                with_finalized_approvals,
                responder,
            } => {
                let ro_txn = self.block_store.checkout_ro()?;

                let transaction = if with_finalized_approvals {
                    match Self::get_transaction_with_finalized_approvals(
                        &ro_txn,
                        &transaction_hash,
                    )? {
                        Some((transaction, maybe_approvals)) => {
                            if let Some(approvals) = maybe_approvals {
                                transaction.with_approvals(approvals)
                            } else {
                                transaction
                            }
                        }
                        None => return Ok(responder.respond(None).ignore()),
                    }
                } else {
                    match ro_txn.read(transaction_hash)? {
                        Some(transaction) => transaction,
                        None => return Ok(responder.respond(None).ignore()),
                    }
                };

                let block_hash_height_and_era: BlockHashHeightAndEra =
                    match ro_txn.read(transaction_hash)? {
                        Some(value) => value,
                        None => return Ok(responder.respond(Some((transaction, None))).ignore()),
                    };

                let execution_result = ro_txn.read(transaction_hash)?;
                let execution_info = ExecutionInfo {
                    block_hash: block_hash_height_and_era.block_hash,
                    block_height: block_hash_height_and_era.block_height,
                    execution_result,
                };

                responder
                    .respond(Some((transaction, Some(execution_info))))
                    .ignore()
            }
            StorageRequest::IsTransactionStored {
                transaction_id,
                responder,
            } => {
                let txn = self.block_store.checkout_ro()?;
                let has_transaction = DataReader::<TransactionHash, Transaction>::exists(
                    &txn,
                    transaction_id.transaction_hash(),
                )?;
                responder.respond(has_transaction).ignore()
            }
            StorageRequest::GetExecutionResults {
                block_hash,
                responder,
            } => {
                let txn = self.block_store.checkout_ro()?;
                responder
                    .respond(Self::get_execution_results_with_transaction_headers(
                        &txn,
                        &block_hash,
                    )?)
                    .ignore()
            }
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
                let mut rw_txn = self.block_store.checkout_rw()?;
                let _ = rw_txn.write(&BlockExecutionResults {
                    block_info: BlockHashHeightAndEra::new(*block_hash, block_height, era_id),
                    exec_results: execution_results,
                })?;
                rw_txn.commit()?;
                responder.respond(()).ignore()
            }
            StorageRequest::GetFinalitySignature { id, responder } => {
                let maybe_sig = self
                    .block_store
                    .checkout_ro()?
                    .read(*id.block_hash())?
                    .and_then(|sigs: BlockSignatures| sigs.finality_signature(id.public_key()))
                    .filter(|sig| sig.era_id() == id.era_id());
                responder.respond(maybe_sig).ignore()
            }
            StorageRequest::IsFinalitySignatureStored { id, responder } => {
                let has_signature = self
                    .block_store
                    .checkout_ro()?
                    .read(*id.block_hash())?
                    .map(|sigs: BlockSignatures| sigs.has_finality_signature(id.public_key()))
                    .unwrap_or(false);
                responder.respond(has_signature).ignore()
            }
            StorageRequest::GetBlockAndMetadataByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            } => {
                if !(self.should_return_block(block_height, only_from_available_block_range)) {
                    return Ok(responder.respond(None).ignore());
                }

                let ro_txn = self.block_store.checkout_ro()?;

                let block: Block = {
                    if let Some(block) = ro_txn.read(block_height)? {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
                };

                let hash = block.hash();
                let block_signatures = match ro_txn.read(*hash)? {
                    Some(signatures) => signatures,
                    None => self.get_default_block_signatures(&block),
                };
                responder
                    .respond(Some(BlockWithMetadata {
                        block,
                        block_signatures,
                    }))
                    .ignore()
            }
            StorageRequest::PutBlockSignatures {
                signatures,
                responder,
            } => {
                if signatures.is_empty() {
                    error!(
                        ?signatures,
                        "should not attempt to store empty collection of block signatures"
                    );
                    return Ok(responder.respond(false).ignore());
                }
                let mut txn = self.block_store.checkout_rw()?;
                let old_data: Option<BlockSignatures> = txn.read(*signatures.block_hash())?;
                let new_data = match old_data {
                    None => signatures,
                    Some(mut data) => {
                        if let Err(error) = data.merge(signatures) {
                            error!(%error, "failed to put block signatures");
                            return Ok(responder.respond(false).ignore());
                        }
                        data
                    }
                };
                let _ = txn.write(&new_data)?;
                txn.commit()?;
                responder.respond(true).ignore()
            }
            StorageRequest::PutFinalitySignature {
                signature,
                responder,
            } => {
                let mut rw_txn = self.block_store.checkout_rw()?;
                let block_hash = signature.block_hash();
                let mut block_signatures: BlockSignatures =
                    if let Some(existing_signatures) = rw_txn.read(*block_hash)? {
                        existing_signatures
                    } else {
                        match &*signature {
                            FinalitySignature::V1(signature) => {
                                BlockSignaturesV1::new(*signature.block_hash(), signature.era_id())
                                    .into()
                            }
                            FinalitySignature::V2(signature) => BlockSignaturesV2::new(
                                *signature.block_hash(),
                                signature.block_height(),
                                signature.era_id(),
                                signature.chain_name_hash(),
                            )
                            .into(),
                        }
                    };
                match (&mut block_signatures, *signature) {
                    (
                        BlockSignatures::V1(ref mut block_signatures),
                        FinalitySignature::V1(signature),
                    ) => {
                        block_signatures.insert_signature(
                            signature.public_key().clone(),
                            *signature.signature(),
                        );
                    }
                    (
                        BlockSignatures::V2(ref mut block_signatures),
                        FinalitySignature::V2(signature),
                    ) => {
                        block_signatures.insert_signature(
                            signature.public_key().clone(),
                            *signature.signature(),
                        );
                    }
                    (block_signatures, signature) => {
                        let mismatch =
                            VariantMismatch(Box::new((block_signatures.clone(), signature)));
                        return Err(FatalStorageError::from(mismatch));
                    }
                }

                let _ = rw_txn.write(&block_signatures);
                rw_txn.commit()?;
                responder.respond(true).ignore()
            }
            StorageRequest::GetBlockSignature {
                block_hash,
                public_key,
                responder,
            } => {
                let maybe_signatures: Option<BlockSignatures> =
                    self.block_store.checkout_ro()?.read(block_hash)?;
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
                let txn = self.block_store.checkout_ro()?;
                let maybe_header = txn.read(LatestSwitchBlock)?;
                responder.respond(maybe_header).ignore()
            }
            StorageRequest::GetSwitchBlockHeaderByEra { era_id, responder } => {
                let txn = self.block_store.checkout_ro()?;
                let maybe_header = txn.read(era_id)?;
                responder.respond(maybe_header).ignore()
            }
            StorageRequest::PutBlockHeader {
                block_header,
                responder,
            } => {
                let mut rw_txn = self.block_store.checkout_rw()?;
                let _ = rw_txn.write(&*block_header)?;
                rw_txn.commit()?;
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
                let transaction_config = self.transaction_config.clone();
                responder
                    .respond(self.put_executed_block(
                        transaction_config,
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
                    let txn = self.block_store.checkout_ro()?;
                    let key_block_header: BlockHeader = match txn.read(key_block_era)? {
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
                let txn = self.block_store.checkout_ro()?;
                let maybe_data: Option<DbRawBytesSpec> = txn.read((db_table_id, key))?;
                match maybe_data {
                    None => responder.respond(None).ignore(),
                    Some(db_raw) => responder.respond(Some(db_raw)).ignore(),
                }
            }
            StorageRequest::GetBlockUtilizationScore {
                era_id,
                block_height,
                switch_block_utilization,
                responder,
            } => {
                let utilization = self.get_block_utilization_score(
                    era_id,
                    block_height,
                    switch_block_utilization,
                );

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
            let txn = self.block_store.checkout_ro()?;
            txn.read(block_height).map_err(FatalStorageError::from)
        }
    }

    pub(crate) fn get_switch_block_by_era_id(
        &self,
        era_id: &EraId,
    ) -> Result<Option<Block>, FatalStorageError> {
        let txn = self.block_store.checkout_ro()?;
        txn.read(*era_id).map_err(FatalStorageError::from)
    }

    /// Retrieves a set of transactions, along with their potential finalized approvals.
    #[allow(clippy::type_complexity)]
    fn get_transactions_with_finalized_approvals<'a>(
        &self,
        transaction_hashes: impl Iterator<Item = &'a TransactionHash>,
    ) -> Result<SmallVec<[Option<(Transaction, Option<BTreeSet<Approval>>)>; 1]>, FatalStorageError>
    {
        let ro_txn = self.block_store.checkout_ro()?;

        transaction_hashes
            .map(|transaction_hash| {
                Self::get_transaction_with_finalized_approvals(&ro_txn, transaction_hash)
                    .map_err(FatalStorageError::from)
            })
            .collect()
    }

    pub(crate) fn put_executed_block(
        &mut self,
        transaction_config: TransactionConfig,
        block: &Block,
        approvals_hashes: &ApprovalsHashes,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, FatalStorageError> {
        let mut txn = self.block_store.checkout_rw()?;
        let era_id = block.era_id();
        let block_utilization_score = block.block_utilization(transaction_config.clone());
        let has_hit_slot_limit = block.has_hit_slot_capacity(transaction_config.clone());
        let block_hash = txn.write(block)?;
        let _ = txn.write(approvals_hashes)?;
        let block_info = BlockHashHeightAndEra::new(block_hash, block.height(), block.era_id());

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
                Some(max_utlization) => *max_utlization,
                None => {
                    // This should never happen as we just created the scores vector to find the
                    // max value
                    warn!("Unable to determine max utilization, marking 0 utilization");
                    0u64
                }
            }
        };

        debug!("Utilization for block is {utilization}");

        let _ = txn.write(&BlockExecutionResults {
            block_info,
            exec_results: execution_results,
        })?;
        txn.commit()?;

        match self.utilization_tracker.get_mut(&era_id) {
            Some(block_score) => {
                block_score.insert(block.height(), utilization);
            }
            None => {
                let mut block_score = BTreeMap::new();
                block_score.insert(block.height(), utilization);
                self.utilization_tracker.insert(era_id, block_score);
            }
        }

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
        let mut rw_txn = self.block_store.checkout_rw()?;
        rw_txn.write(&StateStore {
            key: Cow::Borrowed(COMPLETED_BLOCKS_STORAGE_KEY),
            value: serialized,
        })?;
        rw_txn.commit().map_err(FatalStorageError::from)
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
        let ro_txn = self.block_store.checkout_ro()?;

        let timestamp =
            match DataReader::<LatestSwitchBlock, BlockHeader>::read(&ro_txn, LatestSwitchBlock)? {
                Some(last_era_header) => last_era_header
                    .timestamp()
                    .saturating_sub(self.max_ttl.value()),
                None => Timestamp::now(),
            };

        let mut blocks = Vec::new();
        for sequence in self.completed_blocks.sequences().iter().rev() {
            let hi = sequence.high();
            let low = sequence.low();
            for idx in (low..=hi).rev() {
                let maybe_block: Result<Option<Block>, BlockStoreError> = ro_txn.read(idx);
                match maybe_block {
                    Ok(Some(block)) => {
                        let should_continue = block.timestamp() >= timestamp;
                        blocks.push(block);
                        if false == should_continue {
                            return Ok(blocks);
                        }
                    }
                    Ok(None) => {
                        continue;
                    }
                    Err(err) => return Err(FatalStorageError::BlockStoreError(err)),
                }
            }
        }
        Ok(blocks)
    }

    /// Returns an executable block.
    pub(crate) fn make_executable_block(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<ExecutableBlock>, FatalStorageError> {
        let (block, transactions) =
            match self.read_block_and_finalized_transactions_by_hash(*block_hash)? {
                Some(block_and_finalized_transactions) => block_and_finalized_transactions,
                None => {
                    error!(
                        ?block_hash,
                        "Storage: unable to make_executable_block for  {}", block_hash
                    );
                    return Ok(None);
                }
            };
        let maybe_finalized_approvals: Option<ApprovalsHashes> =
            self.block_store.checkout_ro()?.read(*block.hash())?;
        if let Some(finalized_approvals) = maybe_finalized_approvals {
            if transactions.len() != finalized_approvals.approvals_hashes().len() {
                error!(
                    ?block_hash,
                    "Storage: transaction hashes length mismatch {}", block_hash
                );
                return Err(FatalStorageError::ApprovalsHashesLengthMismatch {
                    block_hash: *block_hash,
                    expected: transactions.len(),
                    actual: finalized_approvals.approvals_hashes().len(),
                });
            }
            for (transaction, hash) in transactions
                .iter()
                .zip(finalized_approvals.approvals_hashes())
            {
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

    /// Retrieves single block and all of its deploys, with the finalized approvals.
    /// If any of the deploys can't be found, returns `Ok(None)`.
    fn read_block_and_finalized_transactions_by_hash(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<(BlockV2, Vec<Transaction>)>, FatalStorageError> {
        let txn = self.block_store.checkout_ro()?;

        let Some(block) = txn.read(block_hash)? else {
            debug!(
                ?block_hash,
                "Storage: read_block_and_finalized_transactions_by_hash failed to get block for {}",
                block_hash
            );
            return Ok(None);
        };

        let Block::V2(block) = block else {
            debug!(
                ?block_hash,
                "Storage: read_block_and_finalized_transactions_by_hash expected block V2 {}",
                block_hash
            );
            return Ok(None);
        };

        let mut transactions = vec![];
        for (transaction, _) in (self
            .get_transactions_with_finalized_approvals(block.all_transactions())?)
        .into_iter()
        .flatten()
        {
            transactions.push(transaction);
        }

        Ok(Some((block, transactions)))
    }

    /// Retrieves the highest complete block header from storage, if one exists. May return an
    /// LMDB error.
    fn get_highest_complete_block_header(&self) -> Result<Option<BlockHeader>, FatalStorageError> {
        let highest_complete_block_height = match self.completed_blocks.highest_sequence() {
            Some(sequence) => sequence.high(),
            None => {
                return Ok(None);
            }
        };

        let txn = self.block_store.checkout_ro()?;
        txn.read(highest_complete_block_height)
            .map_err(FatalStorageError::from)
    }

    /// Retrieves the highest block header with metadata from storage, if one exists. May return an
    /// LMDB error.
    fn get_highest_complete_signed_block_header(
        &self,
        txn: &(impl DataReader<BlockHeight, BlockHeader> + DataReader<BlockHash, BlockSignatures>),
    ) -> Result<Option<SignedBlockHeader>, FatalStorageError> {
        let highest_complete_block_height = match self.completed_blocks.highest_sequence() {
            Some(sequence) => sequence.high(),
            None => {
                return Ok(None);
            }
        };

        let block_header: Option<BlockHeader> = txn.read(highest_complete_block_height)?;
        match block_header {
            Some(header) => {
                let block_header_hash = header.block_hash();
                let block_signatures: BlockSignatures = match txn.read(block_header_hash)? {
                    Some(signatures) => signatures,
                    None => match &header {
                        BlockHeader::V1(header) => BlockSignatures::V1(BlockSignaturesV1::new(
                            header.block_hash(),
                            header.era_id(),
                        )),
                        BlockHeader::V2(header) => BlockSignatures::V2(BlockSignaturesV2::new(
                            header.block_hash(),
                            header.height(),
                            header.era_id(),
                            self.chain_name_hash,
                        )),
                    },
                };
                Ok(Some(SignedBlockHeader::new(header, block_signatures)))
            }
            None => Ok(None),
        }
    }

    /// Retrieves the highest complete block from storage, if one exists. May return an LMDB error.
    pub fn get_highest_complete_block(&self) -> Result<Option<Block>, FatalStorageError> {
        let highest_complete_block_height = match self.highest_complete_block_height() {
            Some(height) => height,
            None => {
                return Ok(None);
            }
        };

        let txn = self.block_store.checkout_ro()?;
        txn.read(highest_complete_block_height)
            .map_err(FatalStorageError::from)
    }

    /// Retrieves a single block header in a given transaction from storage
    /// respecting the possible restriction on whether the block
    /// should be present in the available blocks index.
    fn get_single_block_header_restricted(
        &self,
        txn: &impl DataReader<BlockHash, BlockHeader>,
        block_hash: &BlockHash,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header = match txn.read(*block_hash)? {
            Some(header) => header,
            None => return Ok(None),
        };

        if !(self.should_return_block(block_header.height(), only_from_available_block_range)) {
            return Ok(None);
        }

        Ok(Some(block_header))
    }

    /// Returns headers of complete blocks of the trusted block's ancestors, back to the most
    /// recent switch block.
    fn get_trusted_ancestor_headers(
        &self,
        txn: &impl DataReader<BlockHash, BlockHeader>,
        trusted_block_header: &BlockHeader,
    ) -> Result<Option<Vec<BlockHeader>>, FatalStorageError> {
        if trusted_block_header.is_genesis() {
            return Ok(Some(vec![]));
        }
        let available_block_range = self.get_available_block_range();
        let mut result = vec![];
        let mut current_trusted_block_header = trusted_block_header.clone();
        loop {
            let parent_hash = current_trusted_block_header.parent_hash();
            let parent_block_header: BlockHeader = match txn.read(*parent_hash)? {
                Some(block_header) => block_header,
                None => {
                    warn!(%parent_hash, "block header not found");
                    return Ok(None);
                }
            };

            if !available_block_range.contains(parent_block_header.height()) {
                debug!(%parent_hash, "block header not complete");
                return Ok(None);
            }

            result.push(parent_block_header.clone());
            if parent_block_header.is_switch_block() || parent_block_header.is_genesis() {
                break;
            }
            current_trusted_block_header = parent_block_header;
        }
        Ok(Some(result))
    }

    /// Returns headers of all known switch blocks after the trusted block but before
    /// highest block, with signatures, plus the signed highest block.
    fn get_signed_block_headers(
        &self,
        txn: &(impl DataReader<BlockHash, BlockSignatures> + DataReader<EraId, BlockHeader>),
        trusted_block_header: &BlockHeader,
        highest_signed_block_header: &SignedBlockHeader,
    ) -> Result<Option<Vec<SignedBlockHeader>>, FatalStorageError> {
        if trusted_block_header.block_hash()
            == highest_signed_block_header.block_header().block_hash()
        {
            return Ok(Some(vec![]));
        }

        let start_era_id: u64 = trusted_block_header.next_block_era_id().into();
        let current_era_id: u64 = highest_signed_block_header.block_header().era_id().into();

        let mut result = vec![];

        for era_id in start_era_id..current_era_id {
            let maybe_block_header: Option<BlockHeader> = txn.read(EraId::from(era_id))?;
            match maybe_block_header {
                Some(block_header) => {
                    let block_signatures = match txn.read(block_header.block_hash())? {
                        Some(signatures) => signatures,
                        None => match &block_header {
                            BlockHeader::V1(header) => BlockSignatures::V1(BlockSignaturesV1::new(
                                header.block_hash(),
                                header.era_id(),
                            )),
                            BlockHeader::V2(header) => BlockSignatures::V2(BlockSignaturesV2::new(
                                header.block_hash(),
                                header.height(),
                                header.era_id(),
                                self.chain_name_hash,
                            )),
                        },
                    };
                    result.push(SignedBlockHeader::new(block_header, block_signatures));
                }
                None => return Ok(None),
            }
        }
        result.push(highest_signed_block_header.clone());

        Ok(Some(result))
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
        let mut txn = self.block_store.checkout_rw()?;
        let original_transaction: Transaction = txn.read(*transaction_hash)?.ok_or({
            FatalStorageError::UnexpectedFinalizedApprovals {
                transaction_hash: *transaction_hash,
            }
        })?;

        // Only store the finalized approvals if they are different from the original ones.
        let maybe_existing_finalized_approvals: Option<BTreeSet<Approval>> =
            txn.read(*transaction_hash)?;
        if maybe_existing_finalized_approvals.as_ref() == Some(finalized_approvals) {
            return Ok(false);
        }

        let original_approvals = original_transaction.approvals();
        if &original_approvals != finalized_approvals {
            let _ = txn.write(&TransactionFinalizedApprovals {
                transaction_hash: *transaction_hash,
                finalized_approvals: finalized_approvals.clone(),
            })?;
            txn.commit()?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Retrieves successful transfers associated with block.
    ///
    /// If there is no record of successful transfers for this block, then the list will be built
    /// from the execution results and stored to `transfer_db`.  The record could have been missing
    /// or incorrectly set to an empty collection due to previous synchronization and storage
    /// issues.  See https://github.com/casper-network/casper-node/issues/4255 and
    /// https://github.com/casper-network/casper-node/issues/4268 for further info.
    fn get_transfers(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, FatalStorageError> {
        let mut rw_txn = self.block_store.checkout_rw()?;
        let maybe_transfers: Option<Vec<Transfer>> = rw_txn.read(*block_hash)?;
        if let Some(transfers) = maybe_transfers {
            if !transfers.is_empty() {
                return Ok(Some(transfers));
            }
        }

        let block: Block = match rw_txn.read(*block_hash)? {
            Some(block) => block,
            None => return Ok(None),
        };

        let deploy_hashes: Vec<DeployHash> = match block.clone_body() {
            BlockBody::V1(v1) => v1.deploy_and_transfer_hashes().copied().collect(),
            BlockBody::V2(v2) => v2
                .all_transactions()
                .filter_map(|transaction_hash| match transaction_hash {
                    TransactionHash::Deploy(deploy_hash) => Some(*deploy_hash),
                    TransactionHash::V1(_) => None,
                })
                .collect(),
        };

        let mut transfers: Vec<Transfer> = vec![];
        for deploy_hash in deploy_hashes {
            let transaction_hash = TransactionHash::Deploy(deploy_hash);
            let successful_xfers = match rw_txn.read(transaction_hash)? {
                Some(exec_result) => successful_transfers(&exec_result),
                None => {
                    error!(%deploy_hash, %block_hash, "should have exec result");
                    vec![]
                }
            };
            transfers.extend(successful_xfers);
        }
        rw_txn.write(&BlockTransfers {
            block_hash: *block_hash,
            transfers: transfers.clone(),
        })?;
        rw_txn.commit()?;
        Ok(Some(transfers))
    }

    /// Retrieves a deploy from the deploy store by deploy hash.
    fn get_legacy_deploy(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<LegacyDeploy>, FatalStorageError> {
        let transaction_hash = TransactionHash::from(deploy_hash);
        let txn = self.block_store.checkout_ro()?;
        let transaction =
            match Self::get_transaction_with_finalized_approvals(&txn, &transaction_hash)? {
                Some((transaction, maybe_approvals)) => {
                    if let Some(approvals) = maybe_approvals {
                        transaction.with_approvals(approvals)
                    } else {
                        transaction
                    }
                }
                None => return Ok(None),
            };

        match transaction {
            Transaction::Deploy(deploy) => Ok(Some(LegacyDeploy::from(deploy))),
            transaction @ Transaction::V1(_) => {
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
        let transaction_hash = transaction_id.transaction_hash();
        let txn = self.block_store.checkout_ro()?;

        let maybe_transaction: Option<Transaction> = txn.read(transaction_hash)?;
        let transaction: Transaction = match maybe_transaction {
            None => return Ok(None),
            Some(transaction) if transaction.fetch_id() == transaction_id => {
                return Ok(Some(transaction));
            }
            Some(transaction) => transaction,
        };

        let finalized_approvals = match txn.read(transaction_hash)? {
            None => return Ok(None),
            Some(approvals) => approvals,
        };

        match (
            transaction_id.approvals_hash(),
            finalized_approvals,
            transaction,
        ) {
            (approvals_hash, finalized_approvals, Transaction::Deploy(deploy)) => {
                match ApprovalsHash::compute(&finalized_approvals) {
                    Ok(computed_approvals_hash) if computed_approvals_hash == approvals_hash => {
                        let deploy = deploy.with_approvals(finalized_approvals);
                        Ok(Some(Transaction::from(deploy)))
                    }
                    Ok(_computed_approvals_hash) => Ok(None),
                    Err(error) => {
                        error!(%error, "failed to calculate finalized deploy approvals hash");
                        Err(FatalStorageError::UnexpectedSerializationFailure(error))
                    }
                }
            }
            (approvals_hash, finalized_approvals, Transaction::V1(transaction_v1)) => {
                match ApprovalsHash::compute(&finalized_approvals) {
                    Ok(computed_approvals_hash) if computed_approvals_hash == approvals_hash => {
                        let transaction_v1 = transaction_v1.with_approvals(finalized_approvals);
                        Ok(Some(Transaction::from(transaction_v1)))
                    }
                    Ok(_computed_approvals_hash) => Ok(None),
                    Err(error) => {
                        error!(%error, "failed to calculate finalized transaction approvals hash");
                        Err(FatalStorageError::UnexpectedSerializationFailure(error))
                    }
                }
            }
        }
    }

    /// Retrieves a single transaction along with its finalized approvals.
    #[allow(clippy::type_complexity)]
    fn get_transaction_with_finalized_approvals(
        txn: &(impl DataReader<TransactionHash, Transaction>
              + DataReader<TransactionHash, BTreeSet<Approval>>),
        transaction_hash: &TransactionHash,
    ) -> Result<Option<(Transaction, Option<BTreeSet<Approval>>)>, FatalStorageError> {
        let maybe_transaction: Option<Transaction> = txn.read(*transaction_hash)?;
        let transaction = match maybe_transaction {
            Some(transaction) => transaction,
            None => return Ok(None),
        };

        let maybe_finalized_approvals: Option<BTreeSet<Approval>> = txn.read(*transaction_hash)?;
        let ret = (transaction, maybe_finalized_approvals);

        Ok(Some(ret))
    }

    pub(crate) fn get_sync_leap(
        &self,
        sync_leap_identifier: SyncLeapIdentifier,
    ) -> Result<FetchResponse<SyncLeap, SyncLeapIdentifier>, FatalStorageError> {
        let block_hash = sync_leap_identifier.block_hash();

        let txn = self.block_store.checkout_ro()?;

        let only_from_available_block_range = true;
        let trusted_block_header = match self.get_single_block_header_restricted(
            &txn,
            &block_hash,
            only_from_available_block_range,
        )? {
            Some(trusted_block_header) => trusted_block_header,
            None => return Ok(FetchResponse::NotFound(sync_leap_identifier)),
        };

        let trusted_ancestor_headers =
            match self.get_trusted_ancestor_headers(&txn, &trusted_block_header)? {
                Some(trusted_ancestor_headers) => trusted_ancestor_headers,
                None => return Ok(FetchResponse::NotFound(sync_leap_identifier)),
            };

        // highest block and signatures are not requested
        if sync_leap_identifier.trusted_ancestor_only() {
            return Ok(FetchResponse::Fetched(SyncLeap {
                trusted_ancestor_only: true,
                trusted_block_header,
                trusted_ancestor_headers,
                signed_block_headers: vec![],
            }));
        }

        let highest_complete_block_header =
            match self.get_highest_complete_signed_block_header(&txn)? {
                Some(highest_complete_block_header) => highest_complete_block_header,
                None => return Ok(FetchResponse::NotFound(sync_leap_identifier)),
            };

        if highest_complete_block_header
            .block_header()
            .era_id()
            .saturating_sub(trusted_block_header.era_id().into())
            > self.recent_era_count.into()
        {
            return Ok(FetchResponse::NotProvided(sync_leap_identifier));
        }

        if highest_complete_block_header.block_header().height() == 0 {
            return Ok(FetchResponse::Fetched(SyncLeap {
                trusted_ancestor_only: false,
                trusted_block_header,
                trusted_ancestor_headers: vec![],
                signed_block_headers: vec![],
            }));
        }

        // The `highest_complete_block_header` and `trusted_block_header` are both within the
        // highest complete block range, thus so are all the switch blocks between them.
        if let Some(signed_block_headers) = self.get_signed_block_headers(
            &txn,
            &trusted_block_header,
            &highest_complete_block_header,
        )? {
            return Ok(FetchResponse::Fetched(SyncLeap {
                trusted_ancestor_only: false,
                trusted_block_header,
                trusted_ancestor_headers,
                signed_block_headers,
            }));
        }

        Ok(FetchResponse::NotFound(sync_leap_identifier))
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
                let txn = self
                    .block_store
                    .checkout_ro()
                    .expect("Could not start transaction for lmdb");

                match txn.read(low) {
                    Ok(Some(block)) => match block {
                        Block::V1(_) | Block::V2(_) => {
                            HighestOrphanedBlockResult::Orphan(block.clone_header())
                        }
                    },
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
        let txn = self.block_store.checkout_ro()?;
        if let Some(last_era_header) =
            DataReader::<LatestSwitchBlock, BlockHeader>::read(&txn, LatestSwitchBlock)?
        {
            let mut result = vec![];
            let last_era_id = last_era_header.era_id();
            result.push(last_era_header);
            for era_id in (0..last_era_id.value())
                .rev()
                .take(count as usize)
                .map(EraId::new)
            {
                match txn.read(era_id)? {
                    None => break,
                    Some(header) => result.push(header),
                }
            }
            result.reverse();
            debug!(
                ?result,
                "Storage: read_highest_switch_block_headers count:({})", count
            );
            Ok(result)
        } else {
            Ok(vec![])
        }
    }

    fn read_block_execution_results_or_chunk(
        &self,
        request: &BlockExecutionResultsOrChunkId,
    ) -> Result<Option<BlockExecutionResultsOrChunk>, FatalStorageError> {
        let txn = self.block_store.checkout_ro()?;

        let execution_results = match Self::get_execution_results(&txn, request.block_hash())? {
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
        let ro_txn = self.block_store.checkout_ro()?;

        ro_txn.read(*block_hash).map_err(FatalStorageError::from)
    }

    fn get_execution_results(
        txn: &(impl DataReader<BlockHash, Block> + DataReader<TransactionHash, ExecutionResult>),
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, ExecutionResult)>>, FatalStorageError> {
        let block = txn.read(*block_hash)?;

        let block_body = match block {
            Some(block) => block.take_body(),
            None => return Ok(None),
        };

        let transaction_hashes: Vec<TransactionHash> = match block_body {
            BlockBody::V1(v1) => v1
                .deploy_and_transfer_hashes()
                .map(TransactionHash::from)
                .collect(),
            BlockBody::V2(v2) => v2.all_transactions().copied().collect(),
        };
        let mut execution_results = vec![];
        for transaction_hash in transaction_hashes {
            match txn.read(transaction_hash)? {
                None => {
                    debug!(
                        %block_hash,
                        %transaction_hash,
                        "retrieved block but execution result for given transaction is absent"
                    );
                    return Ok(None);
                }
                Some(execution_result) => {
                    execution_results.push((transaction_hash, execution_result));
                }
            }
        }
        Ok(Some(execution_results))
    }

    #[allow(clippy::type_complexity)]
    fn get_execution_results_with_transaction_headers(
        txn: &(impl DataReader<BlockHash, Block>
              + DataReader<TransactionHash, ExecutionResult>
              + DataReader<TransactionHash, Transaction>),
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, TransactionHeader, ExecutionResult)>>, FatalStorageError>
    {
        let execution_results = match Self::get_execution_results(txn, block_hash)? {
            Some(execution_results) => execution_results,
            None => return Ok(None),
        };

        let mut ret = Vec::with_capacity(execution_results.len());
        for (transaction_hash, execution_result) in execution_results {
            match txn.read(transaction_hash)? {
                None => {
                    error!(
                        %block_hash,
                        %transaction_hash,
                        "missing transaction"
                    );
                    return Ok(None);
                }
                Some(Transaction::Deploy(deploy)) => ret.push((
                    transaction_hash,
                    deploy.take_header().into(),
                    execution_result,
                )),
                Some(Transaction::V1(transaction_v1)) => {
                    ret.push((transaction_hash, (&transaction_v1).into(), execution_result))
                }
            };
        }
        Ok(Some(ret))
    }

    fn get_block_utilization_score(
        &mut self,
        era_id: EraId,
        block_height: u64,
        block_utilization: u64,
    ) -> Option<(u64, u64)> {
        let ret = match self.utilization_tracker.get_mut(&era_id) {
            Some(utilization) => {
                utilization.entry(block_height).or_insert(block_utilization);

                let transaction_count = utilization.values().sum();
                let block_count = utilization.keys().len() as u64;

                Some((transaction_count, block_count))
            }
            None => {
                let mut utilization = BTreeMap::new();
                utilization.insert(block_height, block_utilization);

                self.utilization_tracker.insert(era_id, utilization);

                let block_count = 1u64;
                Some((block_utilization, block_count))
            }
        };

        self.utilization_tracker
            .retain(|key_era_id, _| key_era_id.value() + 2 >= era_id.value());

        ret
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

/// Returns all `Transform::WriteTransfer`s from the execution effects if this is an
/// `ExecutionResult::Success`, or an empty `Vec` if `ExecutionResult::Failure`.
fn successful_transfers(execution_result: &ExecutionResult) -> Vec<Transfer> {
    let mut all_transfers: Vec<Transfer> = vec![];
    match execution_result {
        ExecutionResult::V1(ExecutionResultV1::Success { effect, .. }) => {
            for transform_v1 in &effect.transforms {
                if let execution_result_v1::TransformKindV1::WriteTransfer(transfer_v1) =
                    &transform_v1.transform
                {
                    all_transfers.push(Transfer::V1(transfer_v1.clone()));
                }
            }
        }
        ExecutionResult::V2(ExecutionResultV2 {
            transfers,
            error_message,
            ..
        }) => {
            if error_message.is_none() {
                for transfer in transfers {
                    all_transfers.push(transfer.clone());
                }
            }
            // else no-op: we only record transfers from successful executions.
        }
        ExecutionResult::V1(ExecutionResultV1::Failure { .. }) => {
            // No-op: we only record transfers from successful executions.
        }
    }
    all_transfers
}

// Testing code. The functions below allow direct inspection of the storage component and should
// only ever be used when writing tests.
#[cfg(test)]
impl Storage {
    /// Directly returns a transaction with finalized approvals from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn get_transaction_with_finalized_approvals_by_hash(
        &self,
        transaction_hash: &TransactionHash,
    ) -> Option<(Transaction, Option<BTreeSet<Approval>>)> {
        let txn = self
            .block_store
            .checkout_ro()
            .expect("could not create RO transaction");
        Self::get_transaction_with_finalized_approvals(&txn, transaction_hash)
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
            .checkout_ro()
            .expect("could not create RO transaction")
            .read(*transaction_hash)
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
            .checkout_ro()
            .expect("could not create RO transaction")
            .read(transaction_hash)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_block_by_hash(&self, block_hash: BlockHash) -> Option<Block> {
        self.block_store
            .checkout_ro()
            .expect("could not create RO transaction")
            .read(block_hash)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_block_by_height(&self, height: u64) -> Option<Block> {
        self.block_store
            .checkout_ro()
            .expect("could not create RO transaction")
            .read(height)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_highest_block(&self) -> Option<Block> {
        self.block_store
            .checkout_ro()
            .expect("could not create RO transaction")
            .read(Tip)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_highest_block_header(&self) -> Option<BlockHeader> {
        self.block_store
            .checkout_ro()
            .expect("could not create RO transaction")
            .read(Tip)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn get_finality_signatures_for_block(
        &self,
        block_hash: BlockHash,
    ) -> Option<BlockSignatures> {
        let txn = self
            .block_store
            .checkout_ro()
            .expect("could not create RO transaction");
        let res: Option<BlockSignatures> = txn
            .read(block_hash)
            .expect("could not retrieve value from storage");
        txn.commit().expect("Could not commit transaction");
        res
    }

    pub(crate) fn read_switch_block_by_era_id(&self, era_id: EraId) -> Option<Block> {
        self.block_store
            .checkout_ro()
            .expect("could not create RO transaction")
            .read(era_id)
            .expect("could not retrieve value from storage")
    }

    pub(crate) fn read_signed_block_by_hash(
        &self,
        block_hash: BlockHash,
        only_from_available_block_range: bool,
    ) -> Option<SignedBlock> {
        let ro_txn = self
            .block_store
            .checkout_ro()
            .expect("should create ro txn");
        let block: Block = ro_txn.read(block_hash).expect("should read block")?;

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
        let block_signatures = ro_txn
            .read(block_hash)
            .expect("should read block signatures")
            .unwrap_or_else(|| self.get_default_block_signatures(&block));
        if block_signatures.is_verified().is_err() {
            error!(?block, "invalid block signatures for block");
            debug_assert!(block_signatures.is_verified().is_ok());
            return None;
        }
        Some(SignedBlock::new(block, block_signatures))
    }

    pub(crate) fn read_signed_block_by_height(
        &self,
        height: u64,
        only_from_available_block_range: bool,
    ) -> Option<SignedBlock> {
        if !(self.should_return_block(height, only_from_available_block_range)) {
            return None;
        }
        let ro_txn = self
            .block_store
            .checkout_ro()
            .expect("should create ro txn");
        let block: Block = ro_txn.read(height).expect("should read block")?;
        let hash = block.hash();
        let block_signatures = ro_txn
            .read(*hash)
            .expect("should read block signatures")
            .unwrap_or_else(|| self.get_default_block_signatures(&block));
        Some(SignedBlock::new(block, block_signatures))
    }

    pub(crate) fn read_highest_signed_block(
        &self,
        only_from_available_block_range: bool,
    ) -> Option<SignedBlock> {
        let ro_txn = self
            .block_store
            .checkout_ro()
            .expect("should create ro txn");
        let highest_block = if only_from_available_block_range {
            let height = self.highest_complete_block_height()?;
            ro_txn.read(height).expect("should read block")?
        } else {
            DataReader::<Tip, Block>::read(&ro_txn, Tip).expect("should read block")?
        };
        let hash = highest_block.hash();
        let block_signatures = match ro_txn.read(*hash).expect("should read block signatures") {
            Some(signatures) => signatures,
            None => self.get_default_block_signatures(&highest_block),
        };
        Some(SignedBlock::new(highest_block, block_signatures))
    }

    pub(crate) fn read_execution_info(
        &self,
        transaction_hash: TransactionHash,
    ) -> Option<ExecutionInfo> {
        let txn = self
            .block_store
            .checkout_ro()
            .expect("should create ro txn");
        let block_hash_and_height: BlockHashHeightAndEra = txn
            .read(transaction_hash)
            .expect("should read block hash and height")?;
        let execution_result = txn
            .read(transaction_hash)
            .expect("should read execution result");
        Some(ExecutionInfo {
            block_hash: block_hash_and_height.block_hash,
            block_height: block_hash_and_height.block_height,
            execution_result,
        })
    }
}
