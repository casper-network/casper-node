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

mod block_hash_height_and_era;
mod config;
mod deploy_metadata_v1;
pub(crate) mod disjoint_sequences;
mod error;
mod event;
mod indices;
mod legacy_approvals_hashes;
mod lmdb_ext;
mod metrics;
mod object_pool;
#[cfg(test)]
mod tests;
mod versioned_databases;

#[cfg(test)]
use std::collections::BTreeSet;
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, BTreeMap, HashMap, HashSet},
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};

use datasize::DataSize;
use lmdb::{
    Database, DatabaseFlags, Environment, EnvironmentFlags, RoTransaction, RwCursor, RwTransaction,
    Transaction as LmdbTransaction, WriteFlags,
};
use prometheus::Registry;
use smallvec::SmallVec;
use tracing::{debug, error, info, trace, warn};

#[cfg(test)]
use casper_types::Deploy;
use casper_types::{
    binary_port::{DbRawBytesSpec, RecordId},
    bytesrepr::{FromBytes, ToBytes},
    execution::{
        execution_result_v1, ExecutionResult, ExecutionResultV1, ExecutionResultV2, TransformKind,
    },
    AvailableBlockRange, Block, BlockBody, BlockHash, BlockHeader, BlockSignatures,
    BlockSignaturesV1, BlockSignaturesV2, BlockV2, ChainNameDigest, DeployApprovalsHash,
    DeployHash, Digest, EraId, ExecutionInfo, FinalitySignature, FinalizedApprovals,
    ProtocolVersion, PublicKey, SignedBlock, SignedBlockHeader, StoredValue, Timestamp,
    Transaction, TransactionApprovalsHash, TransactionHash, TransactionHeader, TransactionId,
    TransactionV1ApprovalsHash, Transfer,
};

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
        ApprovalsHashes, BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId,
        BlockWithMetadata, ExecutableBlock, LegacyDeploy, MaxTtl, NodeId, NodeRng, SyncLeap,
        SyncLeapIdentifier, TransactionWithFinalizedApprovals, VariantMismatch,
    },
    utils::{display_error, WithDir},
};
use block_hash_height_and_era::BlockHashHeightAndEra;
pub use config::Config;
use deploy_metadata_v1::DeployMetadataV1;
use disjoint_sequences::{DisjointSequences, Sequence};
pub use error::FatalStorageError;
use error::GetRequestError;
pub(crate) use event::Event;
use legacy_approvals_hashes::LegacyApprovalsHashes;
use lmdb_ext::{BytesreprError, LmdbExtError, TransactionExt, WriteTransactionExt};
use metrics::Metrics;
use object_pool::ObjectPool;
use versioned_databases::VersionedDatabases;

const COMPONENT_NAME: &str = "storage";

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";

/// We can set this very low, as there is only a single reader/writer accessing the component at any
/// one time.
const MAX_TRANSACTIONS: u32 = 1;

/// Maximum number of allowed dbs.
const MAX_DB_COUNT: u32 = 16;
/// Key under which completed blocks are to be stored.
const COMPLETED_BLOCKS_STORAGE_KEY: &[u8] = b"completed_blocks_disjoint_sequences";
/// Name of the file created when initializing a force resync.
const FORCE_RESYNC_FILE_NAME: &str = "force_resync";

/// OS-specific lmdb flags.
#[cfg(not(target_os = "macos"))]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::WRITE_MAP;

/// OS-specific lmdb flags.
///
/// Mac OS X exhibits performance regressions when `WRITE_MAP` is used.
#[cfg(target_os = "macos")]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::empty();

const STORAGE_FILES: [&str; 5] = [
    "data.lmdb",
    "data.lmdb-lock",
    "storage.lmdb",
    "storage.lmdb-lock",
    "sse_index",
];

trait RawDataAccess {
    fn get_raw_bytes(
        &self,
        txn: &RoTransaction,
        key: &[u8],
    ) -> Result<Option<DbRawBytesSpec>, LmdbExtError>;
}

impl RawDataAccess for Database {
    fn get_raw_bytes(
        &self,
        txn: &RoTransaction,
        key: &[u8],
    ) -> Result<Option<DbRawBytesSpec>, LmdbExtError> {
        match txn.get(*self, &key) {
            Ok(raw_bytes) => Ok(Some(DbRawBytesSpec::new_legacy(raw_bytes))),
            Err(lmdb::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

impl Debug for dyn RawDataAccess + Send + 'static {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[RawDataAccess]")
    }
}

/// The storage component.
#[derive(DataSize, Debug)]
pub struct Storage {
    /// Storage location.
    root: PathBuf,
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    env: Arc<Environment>,
    /// The block header databases.
    block_header_dbs: VersionedDatabases<BlockHash, BlockHeader>,
    /// The block body databases.
    block_body_dbs: VersionedDatabases<Digest, BlockBody>,
    /// The approvals hashes databases.
    approvals_hashes_dbs: VersionedDatabases<BlockHash, ApprovalsHashes>,
    /// The block metadata db.
    block_metadata_dbs: VersionedDatabases<BlockHash, BlockSignatures>,
    /// The transaction databases.
    transaction_dbs: VersionedDatabases<TransactionHash, Transaction>,
    /// Databases of `ExecutionResult`s indexed by transaction hash for current DB or by deploy
    /// hash for legacy DB.
    execution_result_dbs: VersionedDatabases<TransactionHash, ExecutionResult>,
    /// The transfer database.
    #[data_size(skip)]
    transfer_db: Database,
    /// The state storage database.
    #[data_size(skip)]
    state_store_db: Database,
    /// The finalized transaction approvals databases.
    finalized_transaction_approvals_dbs: VersionedDatabases<TransactionHash, FinalizedApprovals>,
    /// A map of block height to block ID.
    block_height_index: BTreeMap<u64, BlockHash>,
    /// A map of era ID to switch block ID.
    switch_block_era_id_index: BTreeMap<EraId, BlockHash>,
    /// A map of transaction hashes to hashes, heights and era IDs of blocks containing them.
    transaction_hash_index: BTreeMap<TransactionHash, BlockHashHeightAndEra>,
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
    #[data_size(skip)]
    db_mapper: HashMap<RecordId, Box<dyn RawDataAccess + Send + 'static>>,
    /// The hash of the chain name.
    chain_name_hash: ChainNameDigest,
}

pub(crate) enum HighestOrphanedBlockResult {
    MissingHighestSequence,
    MissingFromBlockHeightIndex(u64),
    Orphan(BlockHeader),
    MissingHeader(BlockHash),
}

impl Display for HighestOrphanedBlockResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            HighestOrphanedBlockResult::MissingHighestSequence => {
                write!(f, "missing highest sequence")
            }
            HighestOrphanedBlockResult::MissingFromBlockHeightIndex(height) => {
                write!(f, "height not found in block height index: {}", height)
            }
            HighestOrphanedBlockResult::Orphan(block_header) => write!(
                f,
                "orphan, height={}, hash={}",
                block_header.height(),
                block_header.block_hash()
            ),
            HighestOrphanedBlockResult::MissingHeader(block_hash) => {
                write!(f, "missing header for block hash: {}", block_hash)
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
            Event::StorageRequest(req) => self.handle_storage_request::<REv>(*req),
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

        // Create the environment and databases.
        let env = new_environment(total_size, root.as_path())?;

        let block_header_dbs: VersionedDatabases<BlockHash, BlockHeader> =
            VersionedDatabases::new(&env, "block_header", "block_header_v2")?;
        let block_metadata_dbs =
            VersionedDatabases::new(&env, "block_metadata", "block_metadata_v2")?;
        let transaction_dbs = VersionedDatabases::new(&env, "deploys", "transactions")?;
        let execution_result_dbs =
            VersionedDatabases::new(&env, "deploy_metadata", "execution_results")?;
        let transfer_db = env.create_db(Some("transfer"), DatabaseFlags::empty())?;
        let state_store_db = env.create_db(Some("state_store"), DatabaseFlags::empty())?;
        let block_body_dbs: VersionedDatabases<Digest, BlockBody> =
            VersionedDatabases::<_, BlockBody>::new(&env, "block_body", "block_body_v2")?;
        let finalized_transaction_approvals_dbs =
            VersionedDatabases::new(&env, "finalized_approvals", "versioned_finalized_approvals")?;
        let approvals_hashes_dbs =
            VersionedDatabases::new(&env, "approvals_hashes", "versioned_approvals_hashes")?;

        let mut db_mapper: HashMap<RecordId, Box<dyn RawDataAccess + Send + 'static>> =
            HashMap::new();
        db_mapper.insert(RecordId::Transaction, Box::new(transaction_dbs));
        db_mapper.insert(RecordId::ExecutionResult, Box::new(execution_result_dbs));
        db_mapper.insert(
            RecordId::FinalizedTransactionApprovals,
            Box::new(finalized_transaction_approvals_dbs),
        );
        db_mapper.insert(RecordId::BlockHeader, Box::new(block_header_dbs));
        db_mapper.insert(RecordId::BlockMetadata, Box::new(block_metadata_dbs));
        db_mapper.insert(RecordId::Transfer, Box::new(transfer_db));
        db_mapper.insert(RecordId::BlockBody, Box::new(block_body_dbs));
        db_mapper.insert(RecordId::ApprovalsHashes, Box::new(approvals_hashes_dbs));

        // We now need to restore the block-height index. Log messages allow timing here.
        info!("indexing block store");
        let mut block_height_index = BTreeMap::new();
        let mut switch_block_era_id_index = BTreeMap::new();
        let mut transaction_hash_index = BTreeMap::new();
        let mut block_txn = env.begin_rw_txn()?;

        let mut deleted_block_hashes = HashSet::new();
        // Map of all block body hashes, with their values representing whether to retain the
        // corresponding block bodies or not.
        let mut block_body_hashes = HashMap::new();
        let mut deleted_transaction_hashes = HashSet::<TransactionHash>::new();

        let mut init_fn = |cursor: &mut RwCursor,
                           block_header: BlockHeader|
         -> Result<(), FatalStorageError> {
            let should_retain_block = match hard_reset_to_start_of_era {
                Some(invalid_era) => {
                    // Retain blocks from eras before the hard reset era, and blocks after this
                    // era if they are from the current protocol version (as otherwise a node
                    // restart would purge them again, despite them being valid).
                    block_header.era_id() < invalid_era
                        || block_header.protocol_version() == protocol_version
                }
                None => true,
            };

            // If we don't already have the block body hash in the collection, insert it with the
            // value `should_retain_block`.
            //
            // If there is an existing value, the updated value should be `false` iff the existing
            // value and `should_retain_block` are both `false`.  Otherwise the updated value should
            // be `true`.
            match block_body_hashes.entry(*block_header.body_hash()) {
                Entry::Vacant(entry) => {
                    entry.insert(should_retain_block);
                }
                Entry::Occupied(entry) => {
                    let value = entry.into_mut();
                    *value = *value || should_retain_block;
                }
            }

            let mut body_txn = env.begin_ro_txn()?;
            let maybe_block_body = block_body_dbs.get(&mut body_txn, block_header.body_hash())?;
            if !should_retain_block {
                let _ = deleted_block_hashes.insert(block_header.block_hash());

                match &maybe_block_body {
                    Some(BlockBody::V1(v1_body)) => deleted_transaction_hashes.extend(
                        v1_body
                            .deploy_and_transfer_hashes()
                            .map(TransactionHash::from),
                    ),
                    Some(BlockBody::V2(v2_body)) => {
                        deleted_transaction_hashes.extend(v2_body.all_transactions().copied())
                    }
                    None => (),
                }

                cursor.del(WriteFlags::empty())?;
                return Ok(());
            }

            Self::insert_to_block_header_indices(
                &mut block_height_index,
                &mut switch_block_era_id_index,
                &block_header,
            )?;

            if let Some(block_body) = maybe_block_body {
                let transaction_hashes = match block_body {
                    BlockBody::V1(v1) => v1
                        .deploy_and_transfer_hashes()
                        .map(TransactionHash::from)
                        .collect(),
                    BlockBody::V2(v2) => v2.all_transactions().copied().collect(),
                };
                Self::insert_to_transaction_index(
                    &mut transaction_hash_index,
                    block_header.block_hash(),
                    block_header.height(),
                    block_header.era_id(),
                    transaction_hashes,
                )?;
            }

            Ok(())
        };

        block_header_dbs.for_each_value_in_current(&mut block_txn, &mut init_fn)?;
        block_header_dbs.for_each_value_in_legacy(&mut block_txn, &mut init_fn)?;

        info!("block store reindexing complete");
        block_txn.commit()?;

        let deleted_block_body_hashes = block_body_hashes
            .into_iter()
            .filter_map(|(body_hash, retain)| (!retain).then_some(body_hash))
            .collect();
        initialize_block_body_dbs(&env, block_body_dbs, deleted_block_body_hashes)?;
        initialize_block_metadata_dbs(&env, block_metadata_dbs, deleted_block_hashes)?;
        initialize_execution_result_dbs(&env, execution_result_dbs, deleted_transaction_hashes)?;

        let metrics = registry.map(Metrics::new).transpose()?;

        let mut component = Self {
            root,
            env: Arc::new(env),
            block_header_dbs,
            block_body_dbs,
            block_metadata_dbs,
            approvals_hashes_dbs,
            transaction_dbs,
            execution_result_dbs,
            transfer_db,
            state_store_db,
            finalized_transaction_approvals_dbs,
            block_height_index,
            switch_block_era_id_index,
            transaction_hash_index,
            completed_blocks: Default::default(),
            activation_era,
            key_block_height_for_activation_point: None,
            enable_mem_deduplication: config.enable_mem_deduplication,
            serialized_item_pool: ObjectPool::new(config.mem_pool_prune_interval),
            recent_era_count,
            max_ttl,
            metrics,
            chain_name_hash: ChainNameDigest::from_chain_name(network_name),
            db_mapper,
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

        match component.read_state_store(&Cow::Borrowed(COMPLETED_BLOCKS_STORAGE_KEY))? {
            Some(raw) => {
                let (mut sequences, _) = DisjointSequences::from_vec(raw)
                    .map_err(FatalStorageError::UnexpectedDeserializationFailure)?;

                // Truncate the sequences in case we removed blocks via a hard reset.
                if let Some(&highest_block_height) = component.block_height_index.keys().last() {
                    sequences.truncate(highest_block_height);
                }

                component.completed_blocks = sequences;
            }
            None => {
                // No state so far. We can make the following observations:
                //
                // 1. Any block already in storage from versions prior to 1.5 (no fast-sync) MUST
                //    have the corresponding global state in contract runtime due to the way sync
                //    worked previously, so with the potential exception of finality signatures, we
                //    can consider all these blocks complete.
                // 2. Any block acquired from that point onwards was subject to the insertion of the
                //    appropriate announcements (`BlockCompletedAnnouncement`), which would have
                //    caused the creation of the completed blocks index, thus would not have
                //    resulted in a `None` value here.
                //
                // Note that a previous run of this version which aborted early could have stored
                // some blocks and/or block-headers without completing the sync process. Hence, when
                // setting the `completed_blocks` in this None case, we'll only consider blocks
                // from a previous protocol version as complete.
                let mut txn = component.env.begin_ro_txn()?;
                for block_hash in component.block_height_index.values().rev() {
                    if let Some(header) = component.get_single_block_header(&mut txn, block_hash)? {
                        if header.protocol_version() < protocol_version {
                            drop(txn);
                            component.completed_blocks =
                                DisjointSequences::new(Sequence::new(0, header.height()));
                            component.persist_completed_blocks()?;
                            break;
                        }
                    }
                }
            }
        }

        Ok(component)
    }

    /// Reads from the state storage database.
    ///
    /// If key is non-empty, returns bytes from under the key. Otherwise returns `Ok(None)`.
    /// May also fail with storage errors.
    fn read_state_store<K: AsRef<[u8]>>(
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

    /// Writes a key to the state storage database.
    // See note below why `key` and `data` are not `&[u8]`s.
    fn write_state_store(
        &self,
        key: Cow<'static, [u8]>,
        data: &Vec<u8>,
    ) -> Result<(), FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;

        // Note: The interface of `lmdb` seems suboptimal: `&K` and `&V` could simply be `&[u8]` for
        //       simplicity. At the very least it seems to be missing a `?Sized` trait bound. For
        //       this reason, we need to use actual sized types in the function signature above.
        txn.put(self.state_store_db, &key, data, WriteFlags::default())?;
        txn.commit()?;

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
                let opt_item = self.read_block(&id).map_err(FatalStorageError::from)?;
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
                    .read_block_header_by_hash(&item_id)
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
                let opt_item =
                    self.read_block_signatures(id.block_hash())?
                        .and_then(|block_signatures| {
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
                let opt_item = self.read_approvals_hashes(&item_id)?;
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
    fn handle_storage_request<REv>(
        &mut self,
        req: StorageRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
        // Note: Database IO is handled in a blocking fashion on purpose throughout this function.
        // The rationale is that long IO operations are very rare and cache misses frequent, so on
        // average the actual execution time will be very low.
        Ok(match req {
            StorageRequest::PutBlock { block, responder } => {
                responder.respond(self.put_block(&block)?).ignore()
            }
            StorageRequest::PutApprovalsHashes {
                approvals_hashes,
                responder,
            } => {
                let env = Arc::clone(&self.env);
                let mut txn = env.begin_rw_txn()?;
                let result = self.write_approvals_hashes(&mut txn, &approvals_hashes)?;
                txn.commit()?;
                responder.respond(result).ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => responder.respond(self.read_block(&block_hash)?).ignore(),
            StorageRequest::IsBlockStored {
                block_hash,
                responder,
            } => responder.respond(self.block_exists(&block_hash)?).ignore(),
            StorageRequest::GetApprovalsHashes {
                block_hash,
                responder,
            } => responder
                .respond(self.read_approvals_hashes(&block_hash)?)
                .ignore(),
            StorageRequest::GetHighestCompleteBlock { responder } => responder
                .respond(self.read_highest_complete_block()?)
                .ignore(),
            StorageRequest::GetHighestCompleteBlockHeader { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_highest_complete_block_header(&mut txn)?)
                    .ignore()
            }
            StorageRequest::GetTransactionsEraIds {
                transaction_hashes,
                responder,
            } => responder
                .respond(self.get_transactions_era_ids(transaction_hashes))
                .ignore(),
            StorageRequest::GetBlockHeader {
                block_hash,
                only_from_available_block_range,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_single_block_header_restricted(
                        &mut txn,
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
            } => responder
                .respond(self.put_transaction(&transaction)?)
                .ignore(),
            StorageRequest::GetTransactions {
                transaction_hashes,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_transactions_with_finalized_approvals(
                        &mut txn,
                        transaction_hashes.iter(),
                    )?)
                    .ignore()
            }
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
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_transaction = match self.get_transaction_with_finalized_approvals(
                    &mut txn,
                    &transaction_id.transaction_hash(),
                )? {
                    None => None,
                    Some(transaction_with_finalized_approvals) => {
                        let transaction = transaction_with_finalized_approvals.into_naive();
                        (transaction.fetch_id() == transaction_id).then_some(transaction)
                    }
                };
                responder.respond(maybe_transaction).ignore()
            }
            StorageRequest::GetTransactionByHash {
                transaction_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_transaction = match self
                    .get_transaction_with_finalized_approvals(&mut txn, &transaction_hash)?
                {
                    None => None,
                    Some(transaction_with_finalized_approvals) => {
                        let transaction = transaction_with_finalized_approvals.into_naive();
                        (transaction.hash() == transaction_hash).then_some(transaction)
                    }
                };
                responder.respond(maybe_transaction).ignore()
            }
            StorageRequest::GetTransactionExecutionInfo {
                transaction_hash,
                responder,
            } => responder
                .respond(self.read_execution_info(transaction_hash)?)
                .ignore(),
            StorageRequest::IsTransactionStored {
                transaction_id,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let has_transaction = self
                    .transaction_dbs
                    .exists(&mut txn, &transaction_id.transaction_hash())?;
                responder.respond(has_transaction).ignore()
            }
            StorageRequest::GetExecutionResults {
                block_hash,
                responder,
            } => responder
                .respond(self.read_execution_results(&block_hash)?)
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
                let env = Arc::clone(&self.env);
                let mut txn = env.begin_rw_txn()?;
                self.write_execution_results(
                    &mut txn,
                    &block_hash,
                    block_height,
                    era_id,
                    execution_results,
                )?;
                txn.commit()?;
                responder.respond(()).ignore()
            }
            StorageRequest::GetFinalitySignature { id, responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_sig = self
                    .get_block_signatures(&mut txn, id.block_hash())?
                    .and_then(|sigs| sigs.finality_signature(id.public_key()))
                    .filter(|sig| sig.era_id() == id.era_id());
                responder.respond(maybe_sig).ignore()
            }
            StorageRequest::IsFinalitySignatureStored { id, responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let has_signature = self
                    .get_block_signatures(&mut txn, id.block_hash())?
                    .map(|sigs| sigs.has_finality_signature(id.public_key()))
                    .unwrap_or(false);
                responder.respond(has_signature).ignore()
            }
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
                    if let Some(block) = self.get_block_by_height(&mut txn, block_height)? {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
                };

                let hash = block.hash();
                let block_signatures = match self.get_block_signatures(&mut txn, hash)? {
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
                let mut txn = self.env.begin_rw_txn()?;
                let old_data: Option<BlockSignatures> = self
                    .block_metadata_dbs
                    .get(&mut txn, signatures.block_hash())?;
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
                let outcome = self.block_metadata_dbs.put(
                    &mut txn,
                    new_data.block_hash(),
                    &new_data,
                    true,
                )?;
                txn.commit()?;
                responder.respond(outcome).ignore()
            }
            StorageRequest::PutFinalitySignature {
                signature,
                responder,
            } => responder
                .respond(self.put_finality_signature(signature)?)
                .ignore(),
            StorageRequest::GetBlockSignature {
                block_hash,
                public_key,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_block_signature(&mut txn, &block_hash, &public_key)?)
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
            StorageRequest::GetSwitchBlockHeaderByEra { era_id, responder } => {
                let maybe_header = self.read_switch_block_header_by_era_id(era_id)?;
                responder.respond(maybe_header).ignore()
            }
            StorageRequest::PutBlockHeader {
                block_header,
                responder,
            } => {
                let result = self.put_block_header(*block_header)?;
                responder.respond(result).ignore()
            }
            StorageRequest::GetAvailableBlockRange { responder } => {
                responder.respond(self.get_available_block_range()).ignore()
            }
            StorageRequest::StoreFinalizedApprovals {
                ref transaction_hash,
                ref finalized_approvals,
                responder,
            } => responder
                .respond(self.store_finalized_approvals(transaction_hash, finalized_approvals)?)
                .ignore(),
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
                    let mut txn = self.env.begin_ro_txn()?;
                    let key_block_era = self.activation_era.predecessor().unwrap_or_default();
                    let key_block_header =
                        match self.get_switch_block_header_by_era_id(&mut txn, key_block_era)? {
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
                let txn = self.env.begin_ro_txn()?;
                if let Some(db) = self.db_mapper.get(&record_id) {
                    responder.respond(db.get_raw_bytes(&txn, key.as_slice())?)
                } else {
                    responder.respond(None)
                }
                .ignore()
            }
        })
    }

    /// Writes a block to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn put_block(&mut self, block: &Block) -> Result<bool, FatalStorageError> {
        let env = Arc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_block(&mut txn, block)?;
        if wrote {
            txn.commit()?;
        }
        Ok(wrote)
    }

    /// Writes a block to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write_block(
        &mut self,
        txn: &mut RwTransaction,
        block: &Block,
    ) -> Result<bool, FatalStorageError> {
        if !self
            .block_body_dbs
            .put(txn, block.body_hash(), &block.clone_body(), true)?
        {
            error!(%block, "could not insert block body");
            return Ok(false);
        }

        let block_header = block.clone_header();
        if !self
            .block_header_dbs
            .put(txn, block.hash(), &block_header, true)?
        {
            error!(%block, "could not insert block header");
            return Ok(false);
        }

        Self::insert_to_block_header_indices(
            &mut self.block_height_index,
            &mut self.switch_block_era_id_index,
            &block_header,
        )?;
        let transaction_hashes = match block {
            Block::V1(v1) => v1
                .deploy_and_transfer_hashes()
                .map(TransactionHash::from)
                .collect(),
            Block::V2(v2) => v2.all_transactions().copied().collect(),
        };
        Self::insert_to_transaction_index(
            &mut self.transaction_hash_index,
            *block.hash(),
            block.height(),
            block.era_id(),
            transaction_hashes,
        )?;

        Ok(true)
    }

    pub(crate) fn put_executed_block(
        &mut self,
        block: &Block,
        approvals_hashes: &ApprovalsHashes,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, FatalStorageError> {
        let env = Arc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_block(&mut txn, block)?;
        if !wrote {
            return Err(FatalStorageError::FailedToOverwriteBlock);
        }

        let _ = self.write_approvals_hashes(&mut txn, approvals_hashes)?;
        let _ = self.write_execution_results(
            &mut txn,
            block.hash(),
            block.height(),
            block.era_id(),
            execution_results,
        )?;
        txn.commit()?;

        Ok(true)
    }

    fn put_finality_signature(
        &mut self,
        signature: Box<FinalitySignature>,
    ) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let mut block_signatures = self
            .block_metadata_dbs
            .get(&mut txn, signature.block_hash())?
            .unwrap_or_else(|| match &*signature {
                FinalitySignature::V1(signature) => {
                    BlockSignaturesV1::new(*signature.block_hash(), signature.era_id()).into()
                }
                FinalitySignature::V2(signature) => BlockSignaturesV2::new(
                    *signature.block_hash(),
                    signature.block_height(),
                    signature.era_id(),
                    signature.chain_name_hash(),
                )
                .into(),
            });

        match (&mut block_signatures, *signature) {
            (BlockSignatures::V1(ref mut block_signatures), FinalitySignature::V1(signature)) => {
                block_signatures
                    .insert_signature(signature.public_key().clone(), *signature.signature());
            }
            (BlockSignatures::V2(ref mut block_signatures), FinalitySignature::V2(signature)) => {
                block_signatures
                    .insert_signature(signature.public_key().clone(), *signature.signature());
            }
            (block_signatures, signature) => {
                let mismatch = VariantMismatch(Box::new((block_signatures.clone(), signature)));
                return Err(FatalStorageError::from(mismatch));
            }
        }
        let outcome = self.block_metadata_dbs.put(
            &mut txn,
            block_signatures.block_hash(),
            &block_signatures,
            true,
        )?;
        txn.commit()?;
        Ok(outcome)
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
        self.write_state_store(Cow::Borrowed(COMPLETED_BLOCKS_STORAGE_KEY), &serialized)
    }

    /// Put a single transaction into storage.
    pub fn put_transaction(&self, transaction: &Transaction) -> Result<bool, FatalStorageError> {
        let transaction_hash = transaction.hash();
        let mut txn = self.env.begin_rw_txn()?;
        let outcome = self
            .transaction_dbs
            .put(&mut txn, &transaction_hash, transaction, false)?;
        if outcome {
            debug!(%transaction_hash, "Storage: new transaction stored");
        } else {
            debug!(%transaction_hash, "Storage: attempt to store existing transaction");
        }
        txn.commit()?;
        Ok(outcome)
    }

    /// Retrieves a block by hash.
    pub fn read_block(&self, block_hash: &BlockHash) -> Result<Option<Block>, FatalStorageError> {
        self.get_single_block(&mut self.env.begin_ro_txn()?, block_hash)
    }

    /// Returns `true` if the given block's header and body are stored.
    fn block_exists(&self, block_hash: &BlockHash) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let block_header = match self.get_single_block_header(&mut txn, block_hash)? {
            Some(block_header) => block_header,
            None => {
                return Ok(false);
            }
        };
        self.block_body_dbs
            .exists(&mut txn, block_header.body_hash())
            .map_err(Into::into)
    }

    /// Retrieves approvals hashes by block hash.
    fn read_approvals_hashes(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let maybe_approvals_hashes = self.approvals_hashes_dbs.get(&mut txn, block_hash)?;
        Ok(maybe_approvals_hashes)
    }

    /// Gets the highest block.
    pub fn read_highest_block(&self) -> Result<Option<Block>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.get_highest_block(&mut txn)
    }

    /// Retrieves the highest block header from the storage, if one exists.
    pub fn read_highest_block_header(&self) -> Result<Option<BlockHeader>, FatalStorageError> {
        let highest_block_hash = match self.block_height_index.iter().last() {
            Some((_, highest_block_hash)) => highest_block_hash,
            None => return Ok(None),
        };
        self.read_block_header_by_hash(highest_block_hash)
    }

    /// Retrieves the height of the highest complete block (if any).
    pub(crate) fn highest_complete_block_height(&self) -> Option<u64> {
        self.completed_blocks
            .highest_sequence()
            .map(|sequence| sequence.high())
    }

    /// Retrieves the highest complete block from the storage, if one exists.
    pub(crate) fn read_highest_complete_block(&self) -> Result<Option<Block>, FatalStorageError> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("Could not start transaction for lmdb");
        let maybe_block = self.get_highest_complete_block(&mut txn)?;
        txn.commit().expect("Could not commit transaction");
        Ok(maybe_block)
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
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("Could not start transaction for lmdb");
        let timestamp = match self.switch_block_era_id_index.keys().last() {
            Some(era_id) => self
                .get_switch_block_header_by_era_id(&mut txn, *era_id)?
                .map(|switch_block| {
                    switch_block
                        .timestamp()
                        .saturating_sub(self.max_ttl.value())
                })
                .unwrap_or_else(Timestamp::now),
            None => Timestamp::now(),
        };

        self.get_blocks_while(&mut txn, |block| block.timestamp() >= timestamp)
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
        if let Some(finalized_approvals) = self.read_approvals_hashes(block.hash())? {
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

    fn write_execution_results(
        &mut self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
        block_height: u64,
        era_id: EraId,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, FatalStorageError> {
        let transaction_hashes = execution_results.keys().copied().collect();
        Self::insert_to_transaction_index(
            &mut self.transaction_hash_index,
            *block_hash,
            block_height,
            era_id,
            transaction_hashes,
        )?;
        let mut transfers: Vec<Transfer> = vec![];
        for (transaction_hash, execution_result) in execution_results.into_iter() {
            transfers.extend(successful_transfers(&execution_result));

            let was_written =
                self.execution_result_dbs
                    .put(txn, &transaction_hash, &execution_result, true)?;

            if !was_written {
                error!(
                    ?block_hash,
                    ?transaction_hash,
                    "failed to write execution results"
                );
                debug_assert!(was_written);
            }
        }

        let was_written = txn.put_value(self.transfer_db, block_hash, &transfers, true)?;
        if !was_written {
            error!(?block_hash, "failed to write transfers");
            debug_assert!(was_written);
        }
        Ok(was_written)
    }

    /// Writes approvals hashes to storage.
    fn write_approvals_hashes(
        &mut self,
        txn: &mut RwTransaction,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<bool, FatalStorageError> {
        let overwrite = true;
        if !self.approvals_hashes_dbs.put(
            txn,
            approvals_hashes.block_hash(),
            approvals_hashes,
            overwrite,
        )? {
            error!("could not insert approvals' hashes: {}", approvals_hashes);
            return Ok(false);
        }
        Ok(true)
    }

    #[cfg(test)]
    pub fn write_finality_signatures(
        &mut self,
        signatures: &BlockSignatures,
    ) -> Result<(), FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let block_hash = signatures.block_hash();
        if self
            .block_metadata_dbs
            .put(&mut txn, block_hash, signatures, true)
            .is_err()
        {
            panic!("write_finality_signatures() failed");
        }
        txn.commit()?;
        Ok(())
    }

    /// Retrieves single switch block by era ID by looking it up in the index and returning it.
    fn get_switch_block_by_era_id<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<Block>, FatalStorageError> {
        self.switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| self.get_single_block(txn, block_hash).transpose())
            .transpose()
    }

    /// Get the switch block for a specified era number in a read-only LMDB database transaction.
    ///
    /// # Panics
    ///
    /// Panics on any IO or db corruption error.
    pub(crate) fn read_switch_block_by_era_id(
        &self,
        era_id: EraId,
    ) -> Result<Option<Block>, FatalStorageError> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("Could not start read only transaction for lmdb");
        let switch_block = self
            .get_switch_block_by_era_id(&mut txn, era_id)
            .expect("LMDB panicked trying to get switch block");
        txn.commit().expect("Could not commit transaction");
        Ok(switch_block)
    }

    /// Returns `count` highest switch block headers, sorted from lowest (oldest) to highest.
    pub(crate) fn read_highest_switch_block_headers(
        &self,
        count: u64,
    ) -> Result<Vec<BlockHeader>, FatalStorageError> {
        let mut result = vec![];
        let mut txn = self.env.begin_ro_txn()?;
        let last_era = self
            .switch_block_era_id_index
            .keys()
            .last()
            .copied()
            .unwrap_or(EraId::new(0));
        for era_id in (0..=last_era.value())
            .into_iter()
            .rev()
            .take(count as usize)
            .map(EraId::new)
        {
            match self.get_switch_block_header_by_era_id(&mut txn, era_id)? {
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
    }

    /// Retrieves the highest block header from the storage, if one exists.
    pub fn read_highest_block_height(&self) -> Option<u64> {
        self.block_height_index.keys().last().copied()
    }

    /// Retrieves a single block header by height by looking it up in the index and returning it.
    pub fn read_block_header_by_height(
        &self,
        height: u64,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let res = self
            .block_height_index
            .get(&height)
            .and_then(|block_hash| {
                self.get_single_block_header(&mut txn, block_hash)
                    .transpose()
            })
            .transpose();
        if !(self.should_return_block(height, only_from_available_block_range)?) {
            return Ok(None);
        }
        res
    }

    /// Retrieves a single block header by hash.
    pub fn read_block_header(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.get_single_block_header(&mut txn, block_hash)
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    pub fn read_block_by_height(&self, height: u64) -> Result<Option<Block>, FatalStorageError> {
        self.get_block_by_height(&mut self.env.begin_ro_txn()?, height)
    }

    /// Retrieves a block by height, together with all stored block signatures.
    ///
    /// Returns `None` if the block is not stored, or if no block signatures are stored for it.
    pub fn read_signed_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<SignedBlock>, FatalStorageError> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create transaction");
        let block = if let Some(block) = self.get_block_by_height(&mut txn, height)? {
            block
        } else {
            return Ok(None);
        };
        let block_signatures =
            if let Some(block_signatures) = self.get_block_signatures(&mut txn, block.hash())? {
                block_signatures
            } else {
                debug!(height, "no block signatures stored for block");
                return Ok(None);
            };
        Ok(Some(SignedBlock::new(block, block_signatures)))
    }

    /// Retrieves single block and all of its deploys, with the finalized approvals.
    /// If any of the deploys can't be found, returns `Ok(None)`.
    fn read_block_and_finalized_transactions_by_hash(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<(BlockV2, Vec<Transaction>)>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;

        let Some(block) = self.get_single_block(&mut txn, &block_hash)? else {
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

        Ok(self
            .get_transactions_with_finalized_approvals(&mut txn, block.all_transactions())?
            .into_iter()
            .map(|maybe_transaction| {
                maybe_transaction.map(TransactionWithFinalizedApprovals::into_naive)
            })
            .collect::<Option<Vec<Transaction>>>()
            .map(|transactions| (block, transactions)))
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    fn get_block_by_height<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        height: u64,
    ) -> Result<Option<Block>, FatalStorageError> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block(txn, block_hash).transpose())
            .transpose()
    }

    /// Retrieves single switch block header by era ID by looking it up in the index and returning
    /// it.
    fn get_switch_block_header_by_era_id<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        trace!(switch_block_era_id_index = ?self.switch_block_era_id_index);
        let ret = self
            .switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| self.get_single_block_header(txn, block_hash).transpose())
            .transpose();
        if let Ok(maybe) = &ret {
            debug!(
                "Storage: get_switch_block_header_by_era_id({:?}) has entry:{}",
                era_id,
                maybe.is_some()
            )
        }
        ret
    }

    /// Retrieves single switch block header by era ID by looking it up in the index and returning
    /// it.
    pub(crate) fn read_switch_block_header_by_era_id(
        &self,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.get_switch_block_header_by_era_id(&mut txn, era_id)
    }

    /// Returns the era IDs of the blocks in which the given transactions were executed.  If none
    /// of the transaction have been executed yet, an empty set will be returned.
    fn get_transactions_era_ids(
        &self,
        transaction_hashes: HashSet<TransactionHash>,
    ) -> HashSet<EraId> {
        transaction_hashes
            .iter()
            .filter_map(|transaction_hash| {
                self.transaction_hash_index
                    .get(transaction_hash)
                    .map(|block_hash_height_and_era| block_hash_height_and_era.era_id)
            })
            .collect()
    }

    /// Retrieves the highest block from storage, if one exists. May return an LMDB error.
    fn get_highest_block<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<Block>, FatalStorageError> {
        self.block_height_index
            .keys()
            .last()
            .and_then(|&height| self.get_block_by_height(txn, height).transpose())
            .transpose()
    }

    /// Retrieves the highest complete block header from storage, if one exists. May return an
    /// LMDB error.
    fn get_highest_complete_block_header<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let highest_complete_block_height = match self.completed_blocks.highest_sequence() {
            Some(sequence) => sequence.high(),
            None => {
                return Ok(None);
            }
        };
        let highest_complete_block_hash =
            match self.block_height_index.get(&highest_complete_block_height) {
                Some(hash) => hash,
                None => {
                    warn!("couldn't find the highest complete block in block height index");
                    return Ok(None);
                }
            };

        // The `completed_blocks` contains blocks with sufficient finality signatures,
        // so we don't need to check the sufficiency again.
        self.get_single_block_header(txn, highest_complete_block_hash)
    }

    /// Retrieves the highest block header with metadata from storage, if one exists. May return an
    /// LMDB error.
    fn get_highest_complete_signed_block_header<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<SignedBlockHeader>, FatalStorageError> {
        let highest_complete_block_height = match self.completed_blocks.highest_sequence() {
            Some(sequence) => sequence.high(),
            None => {
                return Ok(None);
            }
        };
        let highest_complete_block_hash =
            match self.block_height_index.get(&highest_complete_block_height) {
                Some(hash) => hash,
                None => {
                    warn!("couldn't find the highest complete block in block height index");
                    return Ok(None);
                }
            };

        // The `completed_blocks` contains blocks with sufficient finality signatures,
        // so we don't need to check the sufficiency again.
        self.get_single_signed_block_header(txn, highest_complete_block_hash)
    }

    /// Retrieves the highest complete block from storage, if one exists. May return an LMDB error.
    fn get_highest_complete_block<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<Block>, FatalStorageError> {
        let highest_complete_block_height = match self.highest_complete_block_height() {
            Some(height) => height,
            None => {
                return Ok(None);
            }
        };
        let highest_complete_block_hash =
            match self.block_height_index.get(&highest_complete_block_height) {
                Some(hash) => hash,
                None => {
                    warn!("couldn't find the highest complete block in block height index");
                    return Ok(None);
                }
            };

        // The `completed_blocks` contains blocks with sufficient finality signatures,
        // so we don't need to check the sufficiency again.
        self.get_single_block(txn, highest_complete_block_hash)
    }

    /// Returns a vector of blocks that satisfy the predicate, and one that doesn't (if one
    /// exists), starting from the latest one and following the ancestry chain.
    fn get_blocks_while<F, Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        predicate: F,
    ) -> Result<Vec<Block>, FatalStorageError>
    where
        F: Fn(&Block) -> bool,
    {
        let mut blocks = Vec::new();
        for sequence in self.completed_blocks.sequences().iter().rev() {
            let hi = sequence.high();
            let low = sequence.low();
            for idx in (low..=hi).rev() {
                match self.get_block_by_height(txn, idx) {
                    Ok(Some(block)) => {
                        let should_continue = predicate(&block);
                        blocks.push(block);
                        if false == should_continue {
                            return Ok(blocks);
                        }
                    }
                    Ok(None) => {
                        continue;
                    }
                    Err(err) => return Err(err),
                }
            }
        }
        Ok(blocks)
    }

    /// Retrieves a single block header in a given transaction from storage
    /// respecting the possible restriction on whether the block
    /// should be present in the available blocks index.
    fn get_single_block_header_restricted<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header = match self.get_single_block_header(txn, block_hash)? {
            Some(header) => header,
            None => return Ok(None),
        };

        if !(self.should_return_block(block_header.height(), only_from_available_block_range)?) {
            return Ok(None);
        }

        Ok(Some(block_header))
    }

    /// Returns headers of complete blocks of the trusted block's ancestors, back to the most
    /// recent switch block.
    fn get_trusted_ancestor_headers<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
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
            let parent_block_header: BlockHeader =
                match self.get_single_block_header(txn, parent_hash)? {
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
    fn get_signed_block_headers<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
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
            let hash = match self.switch_block_era_id_index.get(&EraId::from(era_id)) {
                Some(hash) => hash,
                None => return Ok(None),
            };

            match self.get_single_signed_block_header(txn, hash)? {
                Some(block) => result.push(block),
                None => return Ok(None),
            }
        }
        result.push(highest_signed_block_header.clone());

        Ok(Some(result))
    }

    /// Retrieves a single block header in a given transaction from storage.
    fn get_single_block_header<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header = match self.block_header_dbs.get(txn, block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        block_header.set_block_hash(*block_hash);
        Ok(Some(block_header))
    }

    /// Retrieves a single block header in a given transaction from storage.
    fn get_single_signed_block_header<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<SignedBlockHeader>, FatalStorageError> {
        let block_header = match self.get_single_block_header(txn, block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let block_header_hash = block_header.block_hash();

        let block_signatures = match self.get_block_signatures(txn, &block_header_hash)? {
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

        Ok(Some(SignedBlockHeader::new(block_header, block_signatures)))
    }

    fn put_block_header(&mut self, block_header: BlockHeader) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let result = self.block_header_dbs.put(
            &mut txn,
            &block_header.block_hash(),
            &block_header,
            false,
        )?;
        txn.commit()?;

        Self::insert_to_block_header_indices(
            &mut self.block_height_index,
            &mut self.switch_block_era_id_index,
            &block_header,
        )?;
        Ok(result)
    }

    /// Retrieves a block header by hash.
    fn read_block_header_by_hash(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let maybe_block_header = self.get_single_block_header(&mut txn, block_hash)?;
        drop(txn);
        Ok(maybe_block_header)
    }

    /// Retrieves a single block in a separate transaction from storage.
    fn get_single_block<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Block>, FatalStorageError> {
        let block_header: BlockHeader = match self.get_single_block_header(txn, block_hash)? {
            Some(block_header) => block_header,
            None => {
                debug!(
                    ?block_hash,
                    "get_single_block: missing block header for {}", block_hash
                );
                return Ok(None);
            }
        };

        let maybe_block_body = self.block_body_dbs.get(txn, block_header.body_hash());
        let block_body = match maybe_block_body? {
            Some(block_body) => block_body,
            None => {
                debug!(
                    ?block_header,
                    "get_single_block: missing block body for {}",
                    block_header.block_hash()
                );
                return Ok(None);
            }
        };
        let block = Block::new_from_header_and_body(block_header, block_body)?;
        Ok(Some(block))
    }

    /// Retrieves successful transfers associated with block.
    ///
    /// If there is no record of successful transfers for this block, then the list will be built
    /// from the execution results and stored to `transfer_db`.  The record could have been missing
    /// or incorrectly set to an empty collection due to previous synchronization and storage
    /// issues.  See https://github.com/casper-network/casper-node/issues/4255 and
    /// https://github.com/casper-network/casper-node/issues/4268 for further info.
    fn get_transfers(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        if let Some(transfers) = txn.get_value::<_, Vec<Transfer>>(self.transfer_db, block_hash)? {
            if !transfers.is_empty() {
                return Ok(Some(transfers));
            }
        }

        let block = match self.get_single_block(&mut txn, block_hash)? {
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
            let successful_xfers =
                match self.execution_result_dbs.get(&mut txn, &transaction_hash)? {
                    Some(exec_result) => successful_transfers(&exec_result),
                    None => {
                        error!(%deploy_hash, %block_hash, "should have exec result");
                        vec![]
                    }
                };
            transfers.extend(successful_xfers);
        }
        txn.put_value(self.transfer_db, block_hash, &transfers, true)?;
        txn.commit()?;
        Ok(Some(transfers))
    }

    /// Retrieves a set of transactions, along with their potential finalized approvals.
    fn get_transactions_with_finalized_approvals<'a, Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        transaction_hashes: impl Iterator<Item = &'a TransactionHash>,
    ) -> Result<SmallVec<[Option<TransactionWithFinalizedApprovals>; 1]>, FatalStorageError> {
        transaction_hashes
            .map(|transaction_hash| {
                self.get_transaction_with_finalized_approvals(txn, transaction_hash)
            })
            .collect()
    }

    /// Retrieves a single transaction along with its finalized approvals.
    fn get_transaction_with_finalized_approvals<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<TransactionWithFinalizedApprovals>, FatalStorageError> {
        let transaction = match self.transaction_dbs.get(txn, transaction_hash)? {
            Some(transaction) => transaction,
            None => return Ok(None),
        };
        let finalized_approvals = self
            .finalized_transaction_approvals_dbs
            .get(txn, transaction_hash)?;

        let ret = match (transaction, finalized_approvals) {
            (
                Transaction::Deploy(deploy),
                Some(FinalizedApprovals::Deploy(finalized_approvals)),
            ) => TransactionWithFinalizedApprovals::new_deploy(deploy, Some(finalized_approvals)),
            (Transaction::Deploy(deploy), None) => {
                TransactionWithFinalizedApprovals::new_deploy(deploy, None)
            }
            (Transaction::V1(transaction), Some(FinalizedApprovals::V1(finalized_approvals))) => {
                TransactionWithFinalizedApprovals::new_v1(transaction, Some(finalized_approvals))
            }
            (Transaction::V1(transaction), None) => {
                TransactionWithFinalizedApprovals::new_v1(transaction, None)
            }
            mismatch => {
                let mismatch = VariantMismatch(Box::new(mismatch));
                error!(%mismatch, "failed getting transaction with finalized approvals");
                return Err(FatalStorageError::from(mismatch));
            }
        };

        Ok(Some(ret))
    }

    /// Retrieves block signatures for a block with a given block hash.
    fn get_block_signatures<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, FatalStorageError> {
        Ok(self.block_metadata_dbs.get(txn, block_hash)?)
    }

    /// Retrieves a finality signature for a block with a given block hash.
    fn get_block_signature<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
        public_key: &PublicKey,
    ) -> Result<Option<FinalitySignature>, FatalStorageError> {
        let maybe_signatures: Option<BlockSignatures> =
            self.block_metadata_dbs.get(txn, block_hash)?;
        Ok(maybe_signatures.and_then(|signatures| signatures.finality_signature(public_key)))
    }

    /// Retrieves block signatures for a block with a given block hash.
    fn read_block_signatures(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.get_block_signatures(&mut txn, block_hash)
    }

    /// Stores a set of finalized approvals if they are different to the approvals in the original
    /// transaction and if they are different to existing finalized approvals if any.
    ///
    /// Returns `true` if the provided approvals were stored.
    fn store_finalized_approvals(
        &self,
        transaction_hash: &TransactionHash,
        finalized_approvals: &FinalizedApprovals,
    ) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let original_transaction = self
            .transaction_dbs
            .get(&mut txn, transaction_hash)?
            .ok_or({
                FatalStorageError::UnexpectedFinalizedApprovals {
                    transaction_hash: *transaction_hash,
                }
            })?;

        // Only store the finalized approvals if they are different from the original ones.
        let maybe_existing_finalized_approvals = self
            .finalized_transaction_approvals_dbs
            .get(&mut txn, transaction_hash)?;
        if maybe_existing_finalized_approvals.as_ref() == Some(finalized_approvals) {
            return Ok(false);
        }

        let should_store = match (original_transaction, finalized_approvals) {
            (
                Transaction::Deploy(original_deploy),
                FinalizedApprovals::Deploy(finalzd_approvals),
            ) => original_deploy.approvals() != finalzd_approvals.inner(),
            (Transaction::V1(original_transaction), FinalizedApprovals::V1(finalzd_approvals)) => {
                original_transaction.approvals() != finalzd_approvals.inner()
            }
            mismatch => {
                let mismatch = VariantMismatch(Box::new((mismatch.0, mismatch.1.clone())));
                error!(%mismatch, "failed storing finalized approvals");
                return Err(FatalStorageError::from(mismatch));
            }
        };

        if should_store {
            let _ = self.finalized_transaction_approvals_dbs.put(
                &mut txn,
                transaction_hash,
                finalized_approvals,
                true,
            )?;
            txn.commit()?;
        }
        Ok(should_store)
    }

    /// Retrieves a deploy from the deploy store by deploy hash.
    fn get_legacy_deploy(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<LegacyDeploy>, FatalStorageError> {
        let transaction_hash = TransactionHash::from(deploy_hash);
        let mut txn = self.env.begin_ro_txn()?;
        let transaction =
            match self.get_transaction_with_finalized_approvals(&mut txn, &transaction_hash)? {
                Some(transaction_with_finalized_approvals) => {
                    transaction_with_finalized_approvals.into_naive()
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
        let mut txn = self.env.begin_ro_txn()?;

        let transaction = match self.transaction_dbs.get(&mut txn, &transaction_hash)? {
            None => return Ok(None),
            Some(transaction) if transaction.fetch_id() == transaction_id => {
                return Ok(Some(transaction));
            }
            Some(transaction) => transaction,
        };

        let finalized_approvals = match self
            .finalized_transaction_approvals_dbs
            .get(&mut txn, &transaction_hash)?
        {
            None => return Ok(None),
            Some(approvals) => approvals,
        };

        match (
            transaction_id.approvals_hash(),
            finalized_approvals,
            transaction,
        ) {
            (
                TransactionApprovalsHash::Deploy(approvals_hash),
                FinalizedApprovals::Deploy(approvals),
                Transaction::Deploy(deploy),
            ) => match DeployApprovalsHash::compute(approvals.inner()) {
                Ok(computed_approvals_hash) if computed_approvals_hash == approvals_hash => {
                    let deploy = deploy.with_approvals(approvals.into_inner());
                    Ok(Some(Transaction::from(deploy)))
                }
                Ok(_computed_approvals_hash) => Ok(None),
                Err(error) => {
                    error!(%error, "failed to calculate finalized deploy approvals hash");
                    Err(LmdbExtError::Other(Box::new(BytesreprError(error))).into())
                }
            },
            (
                TransactionApprovalsHash::V1(approvals_hash),
                FinalizedApprovals::V1(approvals),
                Transaction::V1(transaction_v1),
            ) => match TransactionV1ApprovalsHash::compute(approvals.inner()) {
                Ok(computed_approvals_hash) if computed_approvals_hash == approvals_hash => {
                    let transaction_v1 = transaction_v1.with_approvals(approvals.into_inner());
                    Ok(Some(Transaction::from(transaction_v1)))
                }
                Ok(_computed_approvals_hash) => Ok(None),
                Err(error) => {
                    error!(%error, "failed to calculate finalized transaction approvals hash");
                    Err(LmdbExtError::Other(Box::new(BytesreprError(error))).into())
                }
            },
            mismatch => {
                let mismatch = VariantMismatch(Box::new(mismatch));
                error!(%mismatch, "failed getting transaction by ID");
                Err(FatalStorageError::from(mismatch))
            }
        }
    }

    pub(crate) fn get_sync_leap(
        &self,
        sync_leap_identifier: SyncLeapIdentifier,
    ) -> Result<FetchResponse<SyncLeap, SyncLeapIdentifier>, FatalStorageError> {
        let block_hash = sync_leap_identifier.block_hash();

        let mut txn = self.env.begin_ro_txn()?;

        let only_from_available_block_range = true;
        let trusted_block_header = match self.get_single_block_header_restricted(
            &mut txn,
            &block_hash,
            only_from_available_block_range,
        )? {
            Some(trusted_block_header) => trusted_block_header,
            None => return Ok(FetchResponse::NotFound(sync_leap_identifier)),
        };

        let trusted_ancestor_headers =
            match self.get_trusted_ancestor_headers(&mut txn, &trusted_block_header)? {
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
            match self.get_highest_complete_signed_block_header(&mut txn)? {
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
            &mut txn,
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
    ) -> Result<bool, FatalStorageError> {
        if only_from_available_block_range {
            Ok(self.get_available_block_range().contains(block_height))
        } else {
            Ok(true)
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
                match self.block_height_index.get(&low).cloned() {
                    None => HighestOrphanedBlockResult::MissingFromBlockHeightIndex(low),
                    Some(block_hash) => {
                        let mut txn = self
                            .env
                            .begin_ro_txn()
                            .expect("Could not start transaction for lmdb");
                        match self.get_single_block(&mut txn, &block_hash) {
                            Ok(Some(block)) => match block {
                                Block::V1(_) | Block::V2(_) => {
                                    HighestOrphanedBlockResult::Orphan(block.clone_header())
                                }
                            },
                            Ok(None) | Err(_) => {
                                HighestOrphanedBlockResult::MissingHeader(block_hash)
                            }
                        }
                    }
                }
            }
        }
    }

    fn get_execution_results<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, ExecutionResult)>>, FatalStorageError> {
        let block_header = match self.get_single_block_header(txn, block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let maybe_block_body = self.block_body_dbs.get(txn, block_header.body_hash());

        let Some(block_body) = maybe_block_body? else {
            debug!(
                %block_hash,
                "retrieved block header but block body is absent"
            );
            return Ok(None);
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
            match self.execution_result_dbs.get(txn, &transaction_hash)? {
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
    fn read_execution_results(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, TransactionHeader, ExecutionResult)>>, FatalStorageError>
    {
        let mut txn = self.env.begin_ro_txn()?;
        let execution_results = match self.get_execution_results(&mut txn, block_hash)? {
            Some(execution_results) => execution_results,
            None => return Ok(None),
        };

        let mut ret = Vec::with_capacity(execution_results.len());
        for (transaction_hash, execution_result) in execution_results {
            match self.transaction_dbs.get(&mut txn, &transaction_hash)? {
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
                Some(Transaction::V1(transaction_v1)) => ret.push((
                    transaction_hash,
                    transaction_v1.take_header().into(),
                    execution_result,
                )),
            };
        }
        Ok(Some(ret))
    }

    fn read_block_execution_results_or_chunk(
        &self,
        request: &BlockExecutionResultsOrChunkId,
    ) -> Result<Option<BlockExecutionResultsOrChunk>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let execution_results = match self.get_execution_results(&mut txn, request.block_hash())? {
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

    fn read_execution_info(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<Option<ExecutionInfo>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let Some(block_hash_height_and_era) = self.transaction_hash_index.get(&transaction_hash) else {
            return Ok(None);
        };
        let execution_result = self.execution_result_dbs.get(&mut txn, &transaction_hash)?;
        Ok(Some(ExecutionInfo {
            block_hash: block_hash_height_and_era.block_hash,
            block_height: block_hash_height_and_era.block_height,
            execution_result,
        }))
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
    /// Writes a single deploy into legacy DB.
    pub fn write_legacy_deploy(&self, deploy: &Deploy) -> bool {
        let mut txn = self.env.begin_rw_txn().unwrap();
        let deploy_hash = deploy.hash();
        let outcome = self
            .transaction_dbs
            .put_legacy(&mut txn, deploy_hash, deploy, false);
        txn.commit().unwrap();
        outcome
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
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.transaction_dbs
            .get(&mut txn, &transaction_hash)
            .expect("could not retrieve value from storage")
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
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.execution_result_dbs
            .get(&mut txn, transaction_hash)
            .expect("could not retrieve execution result from storage")
    }

    /// Directly returns a transaction with finalized approvals from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn get_transaction_with_finalized_approvals_by_hash(
        &self,
        transaction_hash: &TransactionHash,
    ) -> Option<TransactionWithFinalizedApprovals> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.get_transaction_with_finalized_approvals(&mut txn, transaction_hash)
            .expect("could not retrieve a transaction with finalized approvals from storage")
    }

    /// Reads all known transaction hashes from the internal store.
    ///
    /// # Panics
    ///
    /// Panics on any IO or db corruption error.
    pub(crate) fn get_all_transaction_hashes(&self) -> BTreeSet<TransactionHash> {
        let txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.transaction_dbs.keys(&txn)
    }

    pub(crate) fn get_finality_signatures_for_block(
        &self,
        block_hash: BlockHash,
    ) -> Option<BlockSignatures> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        let res = self
            .block_metadata_dbs
            .get(&mut txn, &block_hash)
            .expect("could not retrieve value from storage");
        txn.commit().expect("Could not commit transaction");
        res
    }

    pub fn get_signed_block_by_hash(
        &self,
        block_hash: BlockHash,
        only_from_available_block_range: bool,
    ) -> Option<SignedBlock> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");

        let block = self
            .get_single_block(&mut txn, &block_hash)
            .expect("could not retrieve block from storage")?;
        if !(self
            .should_return_block(block.height(), only_from_available_block_range)
            .expect("could not check if block should be returned"))
        {
            return None;
        }
        let block_signatures = self
            .get_block_signatures(&mut txn, &block_hash)
            .expect("could not retrieve signatures from storage")
            .unwrap_or_else(|| self.get_default_block_signatures(&block));
        if block_signatures.is_verified().is_err() {
            error!(?block, "invalid block signatures for block");
            debug_assert!(block_signatures.is_verified().is_ok());
            return None;
        }
        Some(SignedBlock::new(block, block_signatures))
    }

    pub fn get_signed_block_by_height(
        &self,
        height: u64,
        only_from_available_block_range: bool,
    ) -> Option<SignedBlock> {
        if !(self
            .should_return_block(height, only_from_available_block_range)
            .expect("could not check if block should be returned"))
        {
            return None;
        }

        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        let block = self
            .get_block_by_height(&mut txn, height)
            .expect("could not retrieve block from storage")?;
        let hash = block.hash();
        let block_signatures = self
            .get_block_signatures(&mut txn, hash)
            .expect("could not retrieve signatures from storage")
            .unwrap_or_else(|| self.get_default_block_signatures(&block));
        if block_signatures.is_verified().is_err() {
            error!(?block, "invalid block signatures for block");
            debug_assert!(block_signatures.is_verified().is_ok());
            return None;
        }
        Some(SignedBlock::new(block, block_signatures))
    }

    pub fn get_highest_signed_block(
        &self,
        only_from_available_block_range: bool,
    ) -> Option<SignedBlock> {
        let height = if only_from_available_block_range {
            self.highest_complete_block_height()?
        } else {
            self.block_height_index.keys().last().copied()?
        };
        self.get_signed_block_by_height(height, only_from_available_block_range)
    }
}

fn new_environment(total_size: usize, root: &Path) -> Result<Environment, FatalStorageError> {
    Environment::new()
        .set_flags(
            OS_FLAGS
            // We manage our own directory.
            | EnvironmentFlags::NO_SUB_DIR
            // Disable thread local storage, strongly suggested for operation with tokio.
            | EnvironmentFlags::NO_TLS
            // Disable read-ahead. Our data is not stored/read in sequence that would benefit from the read-ahead.
            | EnvironmentFlags::NO_READAHEAD,
        )
        .set_max_readers(MAX_TRANSACTIONS)
        .set_max_dbs(MAX_DB_COUNT)
        .set_map_size(total_size)
        .open(&root.join(STORAGE_DB_FILENAME))
        .map_err(Into::into)
}

/// Purges stale entries from the block body databases.
fn initialize_block_body_dbs(
    env: &Environment,
    block_body_dbs: VersionedDatabases<Digest, BlockBody>,
    deleted_block_body_hashes: HashSet<Digest>,
) -> Result<(), FatalStorageError> {
    info!("initializing block body databases");
    let mut txn = env.begin_rw_txn()?;
    for body_hash in deleted_block_body_hashes {
        block_body_dbs.delete(&mut txn, &body_hash)?;
    }
    txn.commit()?;
    info!("block body database initialized");
    Ok(())
}

/// Purges stale entries from the block metadata database.
fn initialize_block_metadata_dbs(
    env: &Environment,
    block_metadata_dbs: VersionedDatabases<BlockHash, BlockSignatures>,
    deleted_block_hashes: HashSet<BlockHash>,
) -> Result<(), FatalStorageError> {
    let block_count_to_be_deleted = deleted_block_hashes.len();
    info!(
        block_count_to_be_deleted,
        "initializing block metadata database"
    );
    let mut txn = env.begin_rw_txn()?;
    for block_hash in deleted_block_hashes {
        if let Err(error) = block_metadata_dbs.delete(&mut txn, &block_hash) {
            return Err(error.into());
        }
    }
    txn.commit()?;
    info!("block metadata database initialized");
    Ok(())
}

/// Purges stale entries from the execution result databases.
fn initialize_execution_result_dbs(
    env: &Environment,
    execution_result_dbs: VersionedDatabases<TransactionHash, ExecutionResult>,
    deleted_transaction_hashes: HashSet<TransactionHash>,
) -> Result<(), LmdbExtError> {
    let exec_results_count_to_be_deleted = deleted_transaction_hashes.len();
    info!(
        exec_results_count_to_be_deleted,
        "initializing execution result databases"
    );
    let mut txn = env.begin_rw_txn()?;
    for hash in deleted_transaction_hashes {
        execution_result_dbs.delete(&mut txn, &hash)?;
    }
    txn.commit()?;
    info!("execution result databases initialized");
    Ok(())
}

/// Returns all `Transform::WriteTransfer`s from the execution effects if this is an
/// `ExecutionResult::Success`, or an empty `Vec` if `ExecutionResult::Failure`.
fn successful_transfers(execution_result: &ExecutionResult) -> Vec<Transfer> {
    let mut transfers: Vec<Transfer> = vec![];
    match execution_result {
        ExecutionResult::V1(ExecutionResultV1::Success { effect, .. }) => {
            for transform_entry in &effect.transforms {
                if let execution_result_v1::Transform::WriteTransfer(transfer) =
                    &transform_entry.transform
                {
                    transfers.push(*transfer);
                }
            }
        }
        ExecutionResult::V2(ExecutionResultV2::Success { effects, .. }) => {
            for transform in effects.transforms() {
                if let TransformKind::Write(StoredValue::Transfer(transfer)) = transform.kind() {
                    transfers.push(*transfer);
                }
            }
        }
        ExecutionResult::V1(ExecutionResultV1::Failure { .. })
        | ExecutionResult::V2(ExecutionResultV2::Failure { .. }) => {
            // No-op: we only record transfers from successful executions.
        }
    }
    transfers
}
