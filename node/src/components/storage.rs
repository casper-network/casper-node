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
//! ## Errors
//!
//! The storage component itself is panic free and in general reports three classes of errors:
//! Corruption, temporary resource exhaustion and potential bugs.

pub(crate) mod disjoint_sequences;
mod error;
mod lmdb_ext;
mod metrics;
mod object_pool;
#[cfg(test)]
mod tests;

#[cfg(test)]
use std::collections::BTreeSet;
use std::{
    borrow::Cow,
    collections::{btree_map, hash_map, BTreeMap, HashMap, HashSet},
    convert::{TryFrom, TryInto},
    fmt::{self, Display, Formatter},
    fs::{self, OpenOptions},
    io::ErrorKind,
    mem,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;
use itertools::Itertools;
use lmdb::{
    Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, RwTransaction, Transaction,
    WriteFlags,
};
use prometheus::Registry;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use static_assertions::const_assert;
#[cfg(test)]
use tempfile::TempDir;
use tracing::{debug, error, info, trace, warn};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    EraId, ExecutionResult, ProtocolVersion, PublicKey, Timestamp, Transfer, Transform,
};

use crate::{
    components::{
        fetcher::{FetchItem, FetchResponse},
        Component,
    },
    effect::{
        announcements::FatalAnnouncement,
        incoming::{NetRequest, NetRequestIncoming},
        requests::{
            MakeBlockExecutableRequest, MarkBlockCompletedRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    types::{
        ApprovalsHash, ApprovalsHashes, AvailableBlockRange, Block, BlockAndDeploys, BlockBody,
        BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId, BlockHash,
        BlockHashAndHeight, BlockHashHeightAndEra, BlockHeader, BlockHeaderWithMetadata,
        BlockSignatures, BlockWithMetadata, Deploy, DeployHash, DeployHeader, DeployId,
        DeployMetadata, DeployMetadataExt, DeployWithFinalizedApprovals, FinalitySignature,
        FinalizedApprovals, FinalizedBlock, LegacyDeploy, MaxTtl, NodeId, SyncLeap,
        SyncLeapIdentifier, ValueOrChunk,
    },
    utils::{display_error, WithDir},
    NodeRng,
};
use disjoint_sequences::{DisjointSequences, Sequence};
pub use error::FatalStorageError;
use error::GetRequestError;
use lmdb_ext::{BytesreprError, LmdbExtError, TransactionExt, WriteTransactionExt};
use metrics::Metrics;
use object_pool::ObjectPool;

const COMPONENT_NAME: &str = "storage";

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
const MAX_DB_COUNT: u32 = 9;
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
const _STORAGE_EVENT_SIZE: usize = mem::size_of::<Event>();
const_assert!(_STORAGE_EVENT_SIZE <= 32);

type FinalizedBlockAndDeploys = (FinalizedBlock, Vec<Deploy>);

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
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    env: Rc<Environment>,
    /// The block header database.
    #[data_size(skip)]
    block_header_db: Database,
    /// The block body database.
    #[data_size(skip)]
    block_body_db: Database,
    /// The approvals hashes database.
    #[data_size(skip)]
    approvals_hashes_db: Database,
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
    /// A map of block height to block ID.
    block_height_index: BTreeMap<u64, BlockHash>,
    /// A map of era ID to switch block ID.
    switch_block_era_id_index: BTreeMap<EraId, BlockHash>,
    /// A map of deploy hashes to hashes, heights and era IDs of blocks containing them.
    deploy_hash_index: BTreeMap<DeployHash, BlockHashHeightAndEra>,
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
}

/// A storage component event.
#[derive(Debug, From, Serialize)]
#[repr(u8)]
pub(crate) enum Event {
    /// Storage request.
    #[from]
    StorageRequest(Box<StorageRequest>),
    /// Incoming net request.
    NetRequestIncoming(Box<NetRequestIncoming>),
    /// Mark block completed request.
    #[from]
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
    /// Make block executable request.
    #[from]
    MakeBlockExecutableRequest(Box<MakeBlockExecutableRequest>),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::StorageRequest(req) => req.fmt(f),
            Event::NetRequestIncoming(incoming) => incoming.fmt(f),
            Event::MarkBlockCompletedRequest(req) => req.fmt(f),
            Event::MakeBlockExecutableRequest(req) => req.fmt(f),
        }
    }
}

impl From<NetRequestIncoming> for Event {
    #[inline]
    fn from(incoming: NetRequestIncoming) -> Self {
        Event::NetRequestIncoming(Box::new(incoming))
    }
}

impl From<StorageRequest> for Event {
    #[inline]
    fn from(request: StorageRequest) -> Self {
        Event::StorageRequest(Box::new(request))
    }
}

impl From<MakeBlockExecutableRequest> for Event {
    #[inline]
    fn from(request: MakeBlockExecutableRequest) -> Self {
        Event::MakeBlockExecutableRequest(Box::new(request))
    }
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
                // Disable read-ahead. Our data is not stored/read in sequence that would benefit from the read-ahead.
                | EnvironmentFlags::NO_READAHEAD,
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
        let finalized_approvals_db =
            env.create_db(Some("finalized_approvals"), DatabaseFlags::empty())?;
        let block_body_db = env.create_db(Some("block_body"), DatabaseFlags::empty())?;
        let approvals_hashes_db =
            env.create_db(Some("approvals_hashes"), DatabaseFlags::empty())?;

        // We now need to restore the block-height index. Log messages allow timing here.
        info!("indexing block store");
        let mut block_height_index = BTreeMap::new();
        let mut switch_block_era_id_index = BTreeMap::new();
        let mut deploy_hash_index = BTreeMap::new();
        let mut block_txn = env.begin_rw_txn()?;
        let mut cursor = block_txn.open_rw_cursor(block_header_db)?;

        let mut deleted_block_hashes = HashSet::new();
        let mut deleted_block_body_hashes = HashSet::new();
        let mut deleted_deploy_hashes = HashSet::<DeployHash>::new();

        // Note: `iter_start` has an undocumented panic if called on an empty database. We rely on
        //       the iterator being at the start when created.
        for row in cursor.iter() {
            let (_, raw_val) = row?;
            let mut body_txn = env.begin_ro_txn()?;
            let block_header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
            let maybe_block_body =
                get_body_for_block_header(&mut body_txn, block_header.body_hash(), block_body_db);
            if let Some(invalid_era) = hard_reset_to_start_of_era {
                // Remove blocks that are in to-be-upgraded eras, but have obsolete protocol
                // versions - they were most likely created before the upgrade and should be
                // reverted.
                if block_header.era_id() >= invalid_era
                    && block_header.protocol_version() < protocol_version
                {
                    let _ = deleted_block_hashes.insert(block_header.block_hash());

                    if let Some(block_body) = maybe_block_body? {
                        deleted_deploy_hashes.extend(block_body.deploy_hashes());
                        deleted_deploy_hashes.extend(block_body.transfer_hashes());
                    }

                    let _ = deleted_block_body_hashes.insert(*block_header.body_hash());

                    cursor.del(WriteFlags::empty())?;
                    continue;
                }
            }

            insert_to_block_header_indices(
                &mut block_height_index,
                &mut switch_block_era_id_index,
                &block_header,
            )?;

            if let Some(block_body) = maybe_block_body? {
                insert_to_deploy_index(
                    &mut deploy_hash_index,
                    block_header.block_hash(),
                    &block_body,
                    block_header.height(),
                    block_header.era_id(),
                )?;
            }
        }
        info!("block store reindexing complete");
        drop(cursor);
        block_txn.commit()?;

        let deleted_block_hashes_raw = deleted_block_hashes.iter().map(BlockHash::as_ref).collect();

        initialize_block_body_db(
            &env,
            &block_header_db,
            &block_body_db,
            &deleted_block_body_hashes
                .iter()
                .map(Digest::as_ref)
                .collect(),
        )?;

        initialize_block_metadata_db(&env, &block_metadata_db, &deleted_block_hashes_raw)?;
        initialize_deploy_metadata_db(&env, &deploy_metadata_db, &deleted_deploy_hashes)?;

        let metrics = registry.map(Metrics::new).transpose()?;

        let mut component = Self {
            root,
            env: Rc::new(env),
            block_header_db,
            block_body_db,
            block_metadata_db,
            approvals_hashes_db,
            deploy_db,
            deploy_metadata_db,
            transfer_db,
            state_store_db,
            finalized_approvals_db,
            block_height_index,
            switch_block_era_id_index,
            deploy_hash_index,
            completed_blocks: Default::default(),
            activation_era,
            key_block_height_for_activation_point: None,
            enable_mem_deduplication: config.enable_mem_deduplication,
            serialized_item_pool: ObjectPool::new(config.mem_pool_prune_interval),
            recent_era_count,
            max_ttl,
            metrics,
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
            NetRequest::Deploy(ref serialized_id) => {
                let id = decode_item_id::<Deploy>(serialized_id)?;
                let opt_item = self.get_deploy(id).map_err(FatalStorageError::from)?;
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
                let opt_item = self
                    .get_legacy_deploy(id)
                    .map_err(FatalStorageError::from)?;
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
                    self.read_block_signatures(&id.block_hash)?
                        .and_then(|block_signatures| {
                            block_signatures.get_finality_signature(&id.public_key)
                        });

                if let Some(item) = opt_item.as_ref() {
                    if item.block_hash != id.block_hash || item.era_id != id.era_id {
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
    fn handle_storage_request(
        &mut self,
        req: StorageRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
        // Note: Database IO is handled in a blocking fashion on purpose throughout this function.
        // The rationale is that long IO operations are very rare and cache misses frequent, so on
        // average the actual execution time will be very low.
        Ok(match req {
            StorageRequest::PutBlock { block, responder } => {
                responder.respond(self.write_block(&block)?).ignore()
            }
            StorageRequest::PutApprovalsHashes {
                approvals_hashes,
                responder,
            } => {
                let env = Rc::clone(&self.env);
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
            StorageRequest::GetDeploysEraIds {
                deploy_hashes,
                responder,
            } => responder
                .respond(self.get_deploys_era_ids(deploy_hashes))
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
            StorageRequest::PutDeploy { deploy, responder } => {
                responder.respond(self.put_deploy(&deploy)?).ignore()
            }
            StorageRequest::GetDeploys {
                deploy_hashes,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(
                        self.get_deploys_with_finalized_approvals(
                            &mut txn,
                            deploy_hashes.as_slice(),
                        )?,
                    )
                    .ignore()
            }
            StorageRequest::GetLegacyDeploy {
                deploy_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_deploy = self
                    .get_deploy_with_finalized_approvals(&mut txn, &deploy_hash)?
                    .map(|deploy_with_finalized_approvals| {
                        LegacyDeploy::from(deploy_with_finalized_approvals.into_naive())
                    });
                responder.respond(maybe_deploy).ignore()
            }
            StorageRequest::GetDeploy {
                deploy_id,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_deploy = match self
                    .get_deploy_with_finalized_approvals(&mut txn, deploy_id.deploy_hash())?
                {
                    None => None,
                    Some(deploy_with_finalized_approvals) => {
                        let deploy = deploy_with_finalized_approvals.into_naive();
                        (deploy.fetch_id() == deploy_id).then_some(deploy)
                    }
                };
                responder.respond(maybe_deploy).ignore()
            }
            StorageRequest::IsDeployStored {
                deploy_id,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let has_deploy = txn.value_exists(self.deploy_db, deploy_id.deploy_hash())?;
                responder.respond(has_deploy).ignore()
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
                execution_results,
                responder,
            } => {
                let env = Rc::clone(&self.env);
                let mut txn = env.begin_rw_txn()?;
                self.write_execution_results(&mut txn, &block_hash, execution_results)?;
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
                    } else if let Some(block_hash_and_height) =
                        self.get_block_hash_and_height_by_deploy_hash(deploy_hash)?
                    {
                        block_hash_and_height.into()
                    } else {
                        DeployMetadataExt::Empty
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
                let block_signatures = match self.get_block_signatures(&mut txn, &block_hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(block_hash, block.header().era_id()),
                };
                if block_signatures.verify().is_err() {
                    error!(?block, "invalid block signatures for block");
                    debug_assert!(block_signatures.verify().is_ok());
                    return Ok(responder.respond(None).ignore());
                }
                responder
                    .respond(Some(BlockWithMetadata {
                        block,
                        block_signatures,
                    }))
                    .ignore()
            }
            StorageRequest::GetFinalitySignature { id, responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_sig = self
                    .get_block_signatures(&mut txn, &id.block_hash)?
                    .and_then(|sigs| sigs.get_finality_signature(&id.public_key))
                    .filter(|sig| sig.era_id == id.era_id);
                responder.respond(maybe_sig).ignore()
            }
            StorageRequest::IsFinalitySignatureStored { id, responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                let has_signature = self
                    .get_block_signatures(&mut txn, &id.block_hash)?
                    .map(|sigs| sigs.has_finality_signature(&id.public_key))
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
                    None => BlockSignatures::new(*hash, block.header().era_id()),
                };
                responder
                    .respond(Some(BlockWithMetadata {
                        block,
                        block_signatures,
                    }))
                    .ignore()
            }
            StorageRequest::GetHighestBlockWithMetadata {
                only_from_available_block_range,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let maybe_height = if only_from_available_block_range {
                    self.highest_complete_block_height()
                } else {
                    self.block_height_index.keys().last().copied()
                };
                let height = match maybe_height {
                    Some(height) => height,
                    None => return Ok(responder.respond(None).ignore()),
                };
                let highest_block = match self.get_block_by_height(&mut txn, height)? {
                    Some(block) => block,
                    None => return Ok(responder.respond(None).ignore()),
                };
                let hash = highest_block.hash();
                let block_signatures = match self.get_block_signatures(&mut txn, hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(*hash, highest_block.header().era_id()),
                };
                responder
                    .respond(Some(BlockWithMetadata {
                        block: highest_block,
                        block_signatures,
                    }))
                    .ignore()
            }
            StorageRequest::PutBlockSignatures {
                signatures,
                responder,
            } => {
                if signatures.proofs.is_empty() {
                    error!(
                        ?signatures,
                        "should not attempt to store empty collection of block signatures"
                    );
                    return Ok(responder.respond(false).ignore());
                }
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
            StorageRequest::PutBlockHeader {
                block_header,
                responder,
            } => {
                let block_header_hash = block_header.block_hash();
                match self.put_block_headers(vec![*block_header]) {
                    Ok(result) => responder.respond(result).ignore(),
                    Err(err) => {
                        error!(?err, ?block_header_hash, "error when storing block header");
                        return Err(err);
                    }
                }
            }
            StorageRequest::GetAvailableBlockRange { responder } => {
                responder.respond(self.get_available_block_range()).ignore()
            }
            StorageRequest::StoreFinalizedApprovals {
                ref deploy_hash,
                ref finalized_approvals,
                responder,
            } => responder
                .respond(self.store_finalized_approvals(deploy_hash, finalized_approvals)?)
                .ignore(),
            StorageRequest::PutExecutedBlock {
                block,
                approvals_hashes,
                execution_results,
                responder,
            } => responder
                .respond(self.put_executed_block(&block, &approvals_hashes, execution_results)?)
                .ignore(),
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
        })
    }

    fn put_finality_signature(
        &mut self,
        signature: Box<FinalitySignature>,
    ) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let mut block_signatures = txn
            .get_value(self.block_metadata_db, &signature.block_hash)?
            .unwrap_or_else(|| BlockSignatures::new(signature.block_hash, signature.era_id));
        block_signatures.insert_proof(signature.public_key, signature.signature);
        let outcome = txn.put_value(
            self.block_metadata_db,
            &block_signatures.block_hash,
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

    /// Put a single deploy into storage.
    pub fn put_deploy(&self, deploy: &Deploy) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let deploy_hash = deploy.hash();
        let outcome = txn.put_value(self.deploy_db, deploy_hash, deploy, false)?;
        if outcome {
            debug!(%deploy_hash, "Storage: new deploy stored");
        } else {
            debug!(%deploy_hash, "Storage: attempt to store existing deploy");
        }
        txn.commit()?;
        Ok(outcome)
    }

    fn put_executed_block(
        &mut self,
        block: &Block,
        approvals_hashes: &ApprovalsHashes,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) -> Result<bool, FatalStorageError> {
        let env = Rc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_validated_block(&mut txn, block)?;
        if !wrote {
            return Err(FatalStorageError::FailedToOverwriteBlock);
        }

        let _ = self.write_approvals_hashes(&mut txn, approvals_hashes)?;
        let _ = self.write_execution_results(&mut txn, block.hash(), execution_results)?;
        txn.commit()?;

        Ok(true)
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
        Ok(txn.value_exists(self.block_body_db, block_header.body_hash())?)
    }

    /// Retrieves a approvals hashes by block hash.
    fn read_approvals_hashes(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let maybe_approvals_hashes = txn.get_value(self.approvals_hashes_db, &block_hash)?;
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
            .expect("Could not start read only transaction for lmdb");
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
            .expect("Could not start read only transaction for lmdb");
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

    /// Make a finalized block from a executed block, respecting Deploy Approvals.
    pub(crate) fn make_executable_block(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<FinalizedBlockAndDeploys>, FatalStorageError> {
        let BlockAndDeploys { block, deploys } =
            match self.read_block_and_finalized_deploys_by_hash(*block_hash)? {
                Some(block_and_finalized_deploys) => block_and_finalized_deploys,
                None => {
                    error!(
                        ?block_hash,
                        "Storage: unable to make_executable_block for  {}", block_hash
                    );
                    return Ok(None);
                }
            };
        if let Some(finalized_approvals) = self.read_approvals_hashes(block.hash())? {
            if deploys.len() != finalized_approvals.approvals_hashes().len() {
                error!(
                    ?block_hash,
                    "Storage: deploy hashes length mismatch {}", block_hash
                );
                return Err(FatalStorageError::ApprovalsHashesLengthMismatch {
                    block_hash: *block_hash,
                    expected: deploys.len(),
                    actual: finalized_approvals.approvals_hashes().len(),
                });
            }
            for (deploy, hash) in deploys.iter().zip(finalized_approvals.approvals_hashes()) {
                if deploy
                    .approvals_hash()
                    .map_err(FatalStorageError::UnexpectedDeserializationFailure)?
                    == *hash
                {
                    continue;
                }
                // This should be unreachable as the `BlockSynchronizer` should ensure we have the
                // correct approvals before it then calls this method.  By returning `Ok(None)` the
                // node would be stalled at this block, but should eventually sync leap due to lack
                // of progress.  It would then backfill this block without executing it.
                error!(
                    ?block_hash,
                    "Storage: deploy with incorrect approvals for  {}", block_hash
                );
                return Ok(None);
            }
        }
        let finalized_block: FinalizedBlock = block.into();
        info!(
            ?block_hash,
            "Storage: created finalized_block({}) {} with {} deploys",
            finalized_block.height(),
            block_hash,
            deploys.len()
        );
        Ok(Some((finalized_block, deploys)))
    }

    /// Writes a block to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    pub fn write_block(&mut self, block: &Block) -> Result<bool, FatalStorageError> {
        // Validate the block prior to inserting it into the database
        block.verify()?;
        let env = Rc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_validated_block(&mut txn, block)?;
        if wrote {
            txn.commit()?;
        }
        Ok(wrote)
    }

    /// Writes a block to storage and marks it as complete, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    /// This function guarantees that either both the block storing and the `completed_blocks` index
    /// update were successful or that the entire operation was reverted.
    pub fn write_complete_block(&mut self, block: &Block) -> Result<bool, FatalStorageError> {
        // Validate the block prior to inserting it into the database
        block.verify()?;
        let env = Rc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_validated_block(&mut txn, block)?;
        if wrote {
            // Update the `completed_blocks` index only if the block was actually stored.
            let _ = self.mark_block_complete(block.height())?;
            txn.commit()?;
        }
        Ok(wrote)
    }

    fn write_execution_results(
        &mut self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) -> Result<bool, FatalStorageError> {
        let mut transfers: Vec<Transfer> = vec![];
        for (deploy_hash, execution_result) in execution_results {
            transfers.extend(successful_transfers(&execution_result));

            let mut metadata = self
                .get_deploy_metadata(txn, &deploy_hash)?
                .unwrap_or_default();

            // If we have a previous execution result, we can continue if it is the same.
            match metadata.execution_results.entry(*block_hash) {
                hash_map::Entry::Occupied(entry) => {
                    if *entry.get() == execution_result {
                        continue;
                    }
                    *entry.into_mut() = execution_result;
                }
                hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(execution_result);
                }
            }

            let was_written =
                txn.put_value(self.deploy_metadata_db, &deploy_hash, &metadata, true)?;
            if !was_written {
                error!(?block_hash, ?deploy_hash, "failed to write deploy metadata");
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
        if !txn.put_value(
            self.approvals_hashes_db,
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
        let block_hash = signatures.block_hash;
        if txn
            .put_value(self.block_metadata_db, &block_hash, signatures, true)
            .is_err()
        {
            panic!("write_finality_signatures() failed");
        }
        txn.commit()?;
        Ok(())
    }

    /// Writes a block which has already been verified to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write_validated_block(
        &mut self,
        txn: &mut RwTransaction,
        block: &Block,
    ) -> Result<bool, FatalStorageError> {
        {
            let block_body_hash = block.header().body_hash();
            let block_body = block.body();
            if !self.put_single_block_body(txn, block_body_hash, block_body)? {
                error!("could not insert body for: {}", block);
                return Ok(false);
            }
        }

        let overwrite = true;

        if !txn.put_value(
            self.block_header_db,
            block.hash(),
            block.header(),
            overwrite,
        )? {
            error!("could not insert block header for block: {}", block);
            return Ok(false);
        }

        {
            insert_to_block_header_indices(
                &mut self.block_height_index,
                &mut self.switch_block_era_id_index,
                block.header(),
            )?;
            insert_to_deploy_index(
                &mut self.deploy_hash_index,
                *block.hash(),
                block.body(),
                block.header().height(),
                block.header().era_id(),
            )?;
        }
        Ok(true)
    }

    /// Retrieves single switch block by era ID by looking it up in the index and returning it.
    fn get_switch_block_by_era_id<Tx: Transaction>(
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
    pub fn read_block_and_metadata_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockWithMetadata>, FatalStorageError> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
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
        Ok(Some(BlockWithMetadata {
            block,
            block_signatures,
        }))
    }

    /// Retrieves single block and all of its deploys, with the finalized approvals.
    /// If any of the deploys can't be found, returns `Ok(None)`.
    fn read_block_and_finalized_deploys_by_hash(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<BlockAndDeploys>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let block = match self.get_single_block(&mut txn, &block_hash)? {
            Some(block) => block,
            None => {
                debug!(
                    ?block_hash,
                    "Storage: read_block_and_finalized_deploys_by_hash failed to get block for {}",
                    block_hash
                );
                return Ok(None);
            }
        };
        let deploy_hashes = block.deploy_and_transfer_hashes().copied().collect_vec();
        Ok(self
            .get_deploys_with_finalized_approvals(&mut txn, &deploy_hashes)?
            .into_iter()
            .map(|maybe_deploy| maybe_deploy.map(|deploy| deploy.into_naive()))
            .collect::<Option<Vec<Deploy>>>()
            .map(|deploys| BlockAndDeploys { block, deploys }))
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    fn get_block_by_height<Tx: Transaction>(
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
    fn get_switch_block_header_by_era_id<Tx: Transaction>(
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

    /// Returns the era IDs of the blocks in which the given deploys were executed.  If none of the
    /// deploys have been executed yet, an empty set will be returned.
    fn get_deploys_era_ids(&self, deploy_hashes: HashSet<DeployHash>) -> HashSet<EraId> {
        deploy_hashes
            .iter()
            .filter_map(|deploy_hash| {
                self.deploy_hash_index
                    .get(deploy_hash)
                    .map(|block_hash_height_and_era| block_hash_height_and_era.era_id)
            })
            .collect()
    }

    /// Retrieves the block hash and height for a deploy hash by looking it up in the index
    /// and returning it.
    fn get_block_hash_and_height_by_deploy_hash(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<BlockHashAndHeight>, FatalStorageError> {
        Ok(self
            .deploy_hash_index
            .get(&deploy_hash)
            .map(BlockHashAndHeight::from))
    }

    /// Retrieves the highest block from storage, if one exists. May return an LMDB error.
    fn get_highest_block<Tx: Transaction>(
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
    fn get_highest_complete_block_header<Tx: Transaction>(
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
    fn get_header_with_metadata_of_highest_complete_block<Tx: Transaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
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
        self.get_single_block_header_with_metadata(txn, highest_complete_block_hash)
    }

    /// Retrieves the highest complete block from storage, if one exists. May return an LMDB error.
    fn get_highest_complete_block<Tx: Transaction>(
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
    fn get_blocks_while<F, Tx: Transaction>(
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
    fn get_single_block_header_restricted<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header: BlockHeader = match txn.get_value(self.block_header_db, &block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };

        if !(self.should_return_block(block_header.height(), only_from_available_block_range)?) {
            return Ok(None);
        }

        block_header.set_block_hash(*block_hash);
        Ok(Some(block_header))
    }

    /// Returns headers of complete blocks of the trusted block's ancestors, back to the most
    /// recent switch block.
    fn get_trusted_ancestor_headers<Tx: Transaction>(
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
                match txn.get_value(self.block_header_db, &parent_hash)? {
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
    fn get_signed_block_headers<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        trusted_block_header: &BlockHeader,
        highest_signed_block_header: &BlockHeaderWithMetadata,
    ) -> Result<Option<Vec<BlockHeaderWithMetadata>>, FatalStorageError> {
        if trusted_block_header.block_hash()
            == highest_signed_block_header.block_header.block_hash()
        {
            return Ok(Some(vec![]));
        }

        let start_era_id: u64 = trusted_block_header.next_block_era_id().into();
        let current_era_id: u64 = highest_signed_block_header.block_header.era_id().into();

        let mut result = vec![];

        for era_id in start_era_id..current_era_id {
            let hash = match self.switch_block_era_id_index.get(&EraId::from(era_id)) {
                Some(hash) => hash,
                None => return Ok(None),
            };

            match self.get_single_block_header_with_metadata(txn, hash)? {
                Some(block) => result.push(block),
                None => return Ok(None),
            }
        }
        result.push(highest_signed_block_header.clone());

        Ok(Some(result))
    }

    /// Retrieves a single block header in a given transaction from storage.
    fn get_single_block_header<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header: BlockHeader = match txn.get_value(self.block_header_db, &block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        block_header.set_block_hash(*block_hash);
        Ok(Some(block_header))
    }

    /// Retrieves a single block header in a given transaction from storage.
    fn get_single_block_header_with_metadata<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let block_header: BlockHeader = match txn.get_value(self.block_header_db, &block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let block_header_hash = block_header.block_hash();

        let block_signatures = match self.get_block_signatures(txn, &block_header_hash)? {
            Some(signatures) => signatures,
            None => BlockSignatures::new(block_header_hash, block_header.era_id()),
        };

        Ok(Some(BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        }))
    }

    /// Stores block headers in the db and, if successful, updates the in-memory indices.
    /// Returns an error on failure or a boolean indicating whether any of the block headers were
    /// previously known.
    fn put_block_headers(
        &mut self,
        block_headers: Vec<BlockHeader>,
    ) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let mut result = false;

        for block_header in &block_headers {
            let block_header_hash = block_header.block_hash();
            match txn.put_value(
                self.block_header_db,
                &block_header_hash,
                block_header,
                false,
            ) {
                Ok(single_result) => {
                    result = result && single_result;
                }
                Err(err) => {
                    error!(?err, ?block_header_hash, "error when storing block header");
                    txn.abort();
                    return Err(err.into());
                }
            }
        }
        txn.commit()?;
        // Update the indices if and only if we wrote to storage correctly.
        for block_header in &block_headers {
            insert_to_block_header_indices(
                &mut self.block_height_index,
                &mut self.switch_block_era_id_index,
                block_header,
            )?;
        }
        Ok(result)
    }

    /// Writes a single block body in a separate transaction to storage.
    fn put_single_block_body(
        &self,
        txn: &mut RwTransaction,
        block_body_hash: &Digest,
        block_body: &BlockBody,
    ) -> Result<bool, LmdbExtError> {
        txn.put_value(self.block_body_db, block_body_hash, block_body, true)
            .map_err(Into::into)
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
    fn get_single_block<Tx: Transaction>(
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
        let maybe_block_body =
            get_body_for_block_header(txn, block_header.body_hash(), self.block_body_db);
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

        let mut transfers: Vec<Transfer> = vec![];
        for deploy_hash in block.deploy_and_transfer_hashes() {
            let metadata = self
                .get_deploy_metadata(&mut txn, deploy_hash)?
                .unwrap_or_default();

            let successful_xfers = match metadata.execution_results.get(block_hash) {
                Some(exec_result) => successful_transfers(exec_result),
                None => {
                    error!(
                        execution_results = ?metadata.execution_results,
                        %block_hash,
                        "should have exec result"
                    );
                    vec![]
                }
            };
            transfers.extend(successful_xfers);
        }
        txn.put_value(self.transfer_db, block_hash, &transfers, true)?;
        txn.commit()?;
        Ok(Some(transfers))
    }

    /// Retrieves block signatures for a block with a given block hash.
    fn get_block_signatures<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, FatalStorageError> {
        Ok(txn.get_value(self.block_metadata_db, block_hash)?)
    }

    /// Retrieves a finality signature for a block with a given block hash.
    fn get_block_signature<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
        public_key: &PublicKey,
    ) -> Result<Option<FinalitySignature>, FatalStorageError> {
        let maybe_signatures: Option<BlockSignatures> =
            txn.get_value(self.block_metadata_db, block_hash)?;
        Ok(maybe_signatures.and_then(|signatures| signatures.get_finality_signature(public_key)))
    }

    /// Retrieves block signatures for a block with a given block hash.
    fn read_block_signatures(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.get_block_signatures(&mut txn, block_hash)
    }

    /// Directly returns a deploy from internal store.
    pub fn read_deploy_by_hash(
        &self,
        deploy_hash: &DeployHash,
    ) -> Result<Option<Deploy>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        Ok(txn.get_value(self.deploy_db, &deploy_hash)?)
    }

    /// Stores a set of finalized approvals if they are different to the approvals in the original
    /// deploy and if they are different to existing finalized approvals if any.
    ///
    /// Returns `true` if the provided approvals were stored.
    fn store_finalized_approvals(
        &self,
        deploy_hash: &DeployHash,
        finalized_approvals: &FinalizedApprovals,
    ) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let maybe_original_deploy: Option<Deploy> = txn.get_value(self.deploy_db, &deploy_hash)?;
        let original_deploy =
            maybe_original_deploy.ok_or(FatalStorageError::UnexpectedFinalizedApprovals {
                deploy_hash: *deploy_hash,
            })?;

        // Only store the finalized approvals if they are different from the original ones.
        let maybe_existing_finalized_approvals: Option<FinalizedApprovals> =
            txn.get_value(self.finalized_approvals_db, deploy_hash)?;

        let should_store = original_deploy.approvals() != finalized_approvals.inner()
            && maybe_existing_finalized_approvals.as_ref() != Some(finalized_approvals);

        if should_store {
            let _ = txn.put_value(
                self.finalized_approvals_db,
                deploy_hash,
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
    ) -> Result<Option<LegacyDeploy>, LmdbExtError> {
        self.env
            .begin_ro_txn()
            .map_err(Into::into)
            .and_then(|mut txn| txn.get_value(self.deploy_db, &deploy_hash))
    }

    /// Retrieves a deploy from the deploy store by deploy ID.
    fn get_deploy(&self, deploy_id: DeployId) -> Result<Option<Deploy>, LmdbExtError> {
        let mut txn = self.env.begin_ro_txn()?;

        let deploy = match txn.get_value::<_, Deploy>(self.deploy_db, deploy_id.deploy_hash())? {
            None => return Ok(None),
            Some(deploy) if deploy.fetch_id() == deploy_id => return Ok(Some(deploy)),
            Some(deploy) => deploy,
        };

        match txn.get_value(self.finalized_approvals_db, deploy_id.deploy_hash())? {
            Some(approvals) => match ApprovalsHash::compute(&approvals) {
                Ok(approvals_hash) if approvals_hash == *deploy_id.approvals_hash() => {
                    Ok(Some(deploy.with_approvals(approvals)))
                }
                Ok(_approvals_hash) => Ok(None),
                Err(error) => {
                    error!(%error, "failed to calculate finalized approvals hash");
                    Err(LmdbExtError::Other(Box::new(BytesreprError(error))))
                }
            },
            None => Ok(None),
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
            match self.get_header_with_metadata_of_highest_complete_block(&mut txn)? {
                Some(highest_complete_block_header) => highest_complete_block_header,
                None => return Ok(FetchResponse::NotFound(sync_leap_identifier)),
            };

        if highest_complete_block_header
            .block_header
            .era_id()
            .saturating_sub(trusted_block_header.era_id().into())
            > self.recent_era_count.into()
        {
            return Ok(FetchResponse::NotProvided(sync_leap_identifier));
        }

        if highest_complete_block_header.block_header.height() == 0 {
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
                            .expect("Could not start read only transaction for lmdb");
                        if let Ok(Some(block)) = self.get_single_block(&mut txn, &block_hash) {
                            HighestOrphanedBlockResult::Orphan(block.header().clone())
                        } else {
                            HighestOrphanedBlockResult::MissingHeader(block_hash)
                        }
                    }
                }
            }
        }
    }

    fn get_execution_results<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(DeployHash, ExecutionResult)>>, FatalStorageError> {
        // There's no mapping between block_hash -> execution results.
        // We store execution results under the deploy hash for the txn.
        // In order to pull it out, we have to:
        // 1. Find the block header for `block_hash`.
        // 2. Find the block body for `block_header.body_hash`.
        // 3. For every txns in the block's body, we load its deploy metadata.
        // 4. We extract txn's execution results from the `deploy_metadata` for the block
        // we're interested in.
        let block_header: BlockHeader = match self.get_single_block_header(txn, block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let maybe_block_body =
            get_body_for_block_header(txn, block_header.body_hash(), self.block_body_db);
        let block_body = match maybe_block_body? {
            Some(block_body) => block_body,
            None => {
                debug!(
                    %block_hash,
                    "retrieved block header but block body is absent"
                );
                return Ok(None);
            }
        };

        let mut execution_results = vec![];
        for deploy_hash in block_body.deploy_and_transfer_hashes() {
            match self.get_deploy_metadata(txn, deploy_hash)? {
                None => {
                    debug!(
                        %block_hash,
                        %deploy_hash,
                        "retrieved block but deploy is absent"
                    );
                    return Ok(None);
                }
                Some(mut metadata) => {
                    match metadata.execution_results.remove(block_hash) {
                        Some(execution_result) => {
                            execution_results.push((*deploy_hash, execution_result));
                        }
                        None => {
                            // We have the block, we've got the deploy but its metadata doesn't
                            // include the reference to the block. This is an error b/c even though
                            // types seem to allow for a single deploy map to multiple blocks, it
                            // shouldn't happen in practice.
                            error!(
                                %block_hash,
                                %deploy_hash,
                                "missing execution results for a deploy in particular block"
                            );
                            return Ok(None);
                        }
                    }
                }
            }
        }
        Ok(Some(execution_results))
    }

    #[allow(clippy::type_complexity)]
    fn read_execution_results(
        &self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(DeployHash, DeployHeader, ExecutionResult)>>, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let execution_results = match self.get_execution_results(&mut txn, block_hash)? {
            Some(execution_results) => execution_results,
            None => return Ok(None),
        };

        let mut ret = Vec::with_capacity(execution_results.len());
        for (deploy_hash, execution_result) in execution_results {
            match txn.get_value::<_, Deploy>(self.deploy_db, &deploy_hash)? {
                None => {
                    error!(
                        %block_hash,
                        %deploy_hash,
                        "missing deploy"
                    );
                    return Ok(None);
                }
                Some(deploy) => ret.push((deploy_hash, deploy.take_header(), execution_result)),
            };
        }
        Ok(Some(ret))
    }

    fn read_block_execution_results_or_chunk(
        &self,
        request: &BlockExecutionResultsOrChunkId,
    ) -> Result<Option<BlockExecutionResultsOrChunk>, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let execution_results = match self.get_execution_results(&mut txn, request.block_hash())? {
            Some(execution_results) => execution_results
                .into_iter()
                .map(|(_deploy_hash, execution_result)| execution_result)
                .collect(),
            None => return Ok(None),
        };
        let value_or_chunk = match ValueOrChunk::new(execution_results, request.chunk_index()) {
            Ok(value_or_chunk) => value_or_chunk,
            Err(error) => {
                // Failure shouldn't be fatal as the node can continue operating but won't be able
                // to answer this particular query. We choose to return `None` instead, signaling
                // other nodes to not query this one for that data.
                error!(
                    ?request,
                    ?error,
                    "failed to construct `BlockExecutionResultsOrChunk`"
                );
                return Ok(None);
            }
        };
        Ok(Some(request.response(value_or_chunk)))
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

/// Inserts the relevant entries to the two indices.
///
/// If a duplicate entry is encountered, neither index is updated and an error is returned.
fn insert_to_block_header_indices(
    block_height_index: &mut BTreeMap<u64, BlockHash>,
    switch_block_era_id_index: &mut BTreeMap<EraId, BlockHash>,
    block_header: &BlockHeader,
) -> Result<(), FatalStorageError> {
    let block_hash = block_header.block_hash();
    if let Some(first) = block_height_index.get(&block_header.height()) {
        if *first != block_hash {
            return Err(FatalStorageError::DuplicateBlockIndex {
                height: block_header.height(),
                first: *first,
                second: block_hash,
            });
        }
    }

    if block_header.is_switch_block() {
        match switch_block_era_id_index.entry(block_header.era_id()) {
            btree_map::Entry::Vacant(entry) => {
                let _ = entry.insert(block_hash);
            }
            btree_map::Entry::Occupied(entry) => {
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

    let _ = block_height_index.insert(block_header.height(), block_hash);
    Ok(())
}

/// Inserts the relevant entries to the index.
///
/// If a duplicate entry is encountered, index is not updated and an error is returned.
fn insert_to_deploy_index(
    deploy_hash_index: &mut BTreeMap<DeployHash, BlockHashHeightAndEra>,
    block_hash: BlockHash,
    block_body: &BlockBody,
    block_height: u64,
    era_id: EraId,
) -> Result<(), FatalStorageError> {
    if let Some(hash) = block_body.deploy_and_transfer_hashes().find(|hash| {
        deploy_hash_index.get(hash).map_or(false, |existing_value| {
            existing_value.block_hash != block_hash
        })
    }) {
        return Err(FatalStorageError::DuplicateDeployIndex {
            deploy_hash: *hash,
            first: BlockHashAndHeight::from(&deploy_hash_index[hash]),
            second: BlockHashAndHeight::new(block_hash, block_height),
        });
    }

    for hash in block_body.deploy_and_transfer_hashes() {
        deploy_hash_index.insert(
            *hash,
            BlockHashHeightAndEra::new(block_hash, block_height, era_id),
        );
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
    pub max_block_store_size: usize,
    /// The maximum size of the database to use for the deploy store.
    ///
    /// The size should be a multiple of the OS page size.
    pub max_deploy_store_size: usize,
    /// The maximum size of the database to use for the deploy metadata store.
    ///
    /// The size should be a multiple of the OS page size.
    pub max_deploy_metadata_store_size: usize,
    /// The maximum size of the database to use for the component state store.
    ///
    /// The size should be a multiple of the OS page size.
    pub max_state_store_size: usize,
    /// Whether or not memory deduplication is enabled.
    pub enable_mem_deduplication: bool,
    /// How many loads before memory duplication checks for dead references.
    pub mem_pool_prune_interval: u16,
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
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        txn.get_value(self.deploy_db, &deploy_hash)
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
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.get_deploy_metadata(&mut txn, deploy_hash)
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
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        self.get_deploy_with_finalized_approvals(&mut txn, deploy_hash)
            .expect("could not retrieve a deploy with finalized approvals from storage")
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
            .map(Result::unwrap)
            .map(|(raw_key, _)| {
                DeployHash::new(Digest::try_from(raw_key).expect("malformed deploy hash in DB"))
            })
            .collect()
    }

    /// Directly returns a deploy from internal store.
    ///
    /// # Panics
    ///
    /// Panics if an IO error occurs.
    pub(crate) fn get_finality_signatures_for_block(
        &self,
        block_hash: BlockHash,
    ) -> Option<BlockSignatures> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        let res = txn
            .get_value(self.block_metadata_db, &block_hash)
            .expect("could not retrieve value from storage");
        txn.commit().expect("Could not commit transaction");
        res
    }
}

fn construct_block_body_to_block_header_reverse_lookup(
    txn: &impl Transaction,
    block_header_db: &Database,
) -> Result<BTreeMap<Digest, BlockHeader>, LmdbExtError> {
    let mut block_body_hash_to_header_map: BTreeMap<Digest, BlockHeader> = BTreeMap::new();
    for row in txn.open_ro_cursor(*block_header_db)?.iter() {
        let (_raw_key, raw_val) = row?;
        let block_header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
        block_body_hash_to_header_map.insert(block_header.body_hash().to_owned(), block_header);
    }
    Ok(block_body_hash_to_header_map)
}

/// Purges stale entries from the block body database.
fn initialize_block_body_db(
    env: &Environment,
    block_header_db: &Database,
    block_body_db: &Database,
    deleted_block_body_hashes_raw: &HashSet<&[u8]>,
) -> Result<(), FatalStorageError> {
    info!("initializing block body database");
    let mut txn = env.begin_rw_txn()?;

    let block_body_hash_to_header_map =
        construct_block_body_to_block_header_reverse_lookup(&txn, block_header_db)?;

    let mut cursor = txn.open_rw_cursor(*block_body_db)?;

    for row in cursor.iter() {
        let (raw_key, _raw_val) = row?;
        let block_body_hash =
            Digest::try_from(raw_key).map_err(|err| LmdbExtError::DataCorrupted(Box::new(err)))?;
        if !block_body_hash_to_header_map.contains_key(&block_body_hash) {
            if !deleted_block_body_hashes_raw.contains(raw_key) {
                // This means that the block body isn't referenced by any header, but no header
                // referencing it was just deleted, either
                warn!(?raw_key, "orphaned block body detected");
            }
            info!(?raw_key, "deleting block body");
            cursor.del(WriteFlags::empty())?;
        }
    }

    drop(cursor);

    txn.commit()?;
    info!("block body database initialized");
    Ok(())
}

/// Retrieves the block body for the given block header.
fn get_body_for_block_header<Tx: Transaction>(
    txn: &mut Tx,
    block_body_hash: &Digest,
    block_body_db: Database,
) -> Result<Option<BlockBody>, LmdbExtError> {
    txn.get_value(block_body_db, block_body_hash)
}

/// Purges stale entries from the block metadata database.
fn initialize_block_metadata_db(
    env: &Environment,
    block_metadata_db: &Database,
    deleted_block_hashes: &HashSet<&[u8]>,
) -> Result<(), FatalStorageError> {
    let block_count_to_be_deleted = deleted_block_hashes.len();
    info!(
        block_count_to_be_deleted,
        "initializing block metadata database"
    );

    if !deleted_block_hashes.is_empty() {
        let mut txn = env.begin_rw_txn()?;
        let mut cursor = txn.open_rw_cursor(*block_metadata_db)?;

        for row in cursor.iter() {
            let (raw_key, _) = row?;
            if deleted_block_hashes.contains(raw_key) {
                cursor.del(WriteFlags::empty())?;
                let digest = Digest::try_from(raw_key);
                debug!(
                    "purged metadata for block {}",
                    digest.map_or("<unknown>".to_string(), |digest| digest.to_string())
                );
                continue;
            }
        }
        drop(cursor);
        txn.commit()?;
    }

    info!("block metadata database initialized");
    Ok(())
}

/// Purges stale entries from the deploy metadata database.
fn initialize_deploy_metadata_db(
    env: &Environment,
    deploy_metadata_db: &Database,
    deleted_deploy_hashes: &HashSet<DeployHash>,
) -> Result<(), LmdbExtError> {
    let deploy_count_to_be_deleted = deleted_deploy_hashes.len();
    info!(
        deploy_count_to_be_deleted,
        "initializing deploy metadata database"
    );

    if !deleted_deploy_hashes.is_empty() {
        let mut txn = env.begin_rw_txn()?;
        deleted_deploy_hashes.iter().for_each(|deleted_deploy_hash| {
        if txn.del(*deploy_metadata_db, deleted_deploy_hash, None).is_err() {
            debug!(%deleted_deploy_hash, "not purging from 'deploy_metadata_db' because not existing");
        }});
        txn.commit()?;
    }

    info!("deploy metadata database initialized");
    Ok(())
}

/// Returns all `Transform::WriteTransfer`s from the execution effects if this is an
/// `ExecutionResult::Success`, or an empty `Vec` if `ExecutionResult::Failure`.
pub fn successful_transfers(execution_result: &ExecutionResult) -> Vec<Transfer> {
    let effects = match execution_result {
        ExecutionResult::Success { effect, .. } => effect,
        ExecutionResult::Failure { .. } => return vec![],
    };

    effects
        .transforms
        .iter()
        .filter_map(|transform_entry| {
            if let Transform::WriteTransfer(transfer) = transform_entry.transform {
                Some(transfer)
            } else {
                None
            }
        })
        .collect()
}
