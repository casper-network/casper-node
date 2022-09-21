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
mod object_pool;
#[cfg(test)]
mod tests;

#[cfg(test)]
use std::collections::BTreeSet;
use std::{
    borrow::Cow,
    collections::{btree_map::Entry, BTreeMap, HashSet},
    convert::TryFrom,
    fmt::{self, Display, Formatter},
    fs, mem,
    path::{Path, PathBuf},
    sync::Arc,
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
#[cfg(test)]
use tempfile::TempDir;
use tracing::{debug, error, info, warn};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    EraId, ExecutionResult, ProtocolVersion, PublicKey, TimeDiff, Transfer, Transform,
};

// The reactor! macro needs this in the fetcher tests
pub(crate) use crate::effect::requests::StorageRequest;
use crate::{
    components::{fetcher::FetchResponse, Component},
    effect::{
        incoming::{NetRequest, NetRequestIncoming},
        requests::{MarkBlockCompletedRequest, NetworkRequest, StateStoreRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    reactor::ReactorEvent,
    types::{
        AvailableBlockRange, Block, BlockAndDeploys, BlockBody, BlockDeployApprovals, BlockHash,
        BlockHashAndHeight, BlockHeader, BlockHeaderWithMetadata, BlockHeadersBatch,
        BlockHeadersBatchId, BlockSignatures, BlockWithMetadata, Deploy, DeployHash,
        DeployMetadata, DeployMetadataExt, DeployWithFinalizedApprovals, FetcherItem,
        FinalitySignature, FinalizedApprovals, Item, NodeId, SyncLeap,
    },
    utils::{display_error, WithDir},
    NodeRng,
};
use disjoint_sequences::{DisjointSequences, Sequence};
pub use error::FatalStorageError;
use error::GetRequestError;
use lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt};
use object_pool::ObjectPool;

use super::linear_chain::{self};

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
const MAX_DB_COUNT: u32 = 8;
/// Key under which completed blocks are to be stored.
const COMPLETED_BLOCKS_STORAGE_KEY: &[u8] = b"completed_blocks_disjoint_sequences";

/// OS-specific lmdb flags.
#[cfg(not(target_os = "macos"))]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::WRITE_MAP;

/// OS-specific lmdb flags.
///
/// Mac OS X exhibits performance regressions when `WRITE_MAP` is used.
#[cfg(target_os = "macos")]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::empty();
const _STORAGE_EVENT_SIZE: usize = mem::size_of::<Event>();
// const_assert!(_STORAGE_EVENT_SIZE <= 96);

#[test]
fn size() {
    todo!("fix _STORAGE_EVENT_SIZE check")
}

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
    env: Environment,
    /// The block header database.
    #[data_size(skip)]
    block_header_db: Database,
    /// The block body database.
    #[data_size(skip)]
    block_body_db: Database,
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
    /// A map of deploy hashes to hashes and heights of blocks containing them.
    deploy_hash_index: BTreeMap<DeployHash, BlockHashAndHeight>,
    /// Runs of completed blocks known in storage.
    completed_blocks: DisjointSequences,
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
    fault_tolerance_fraction: Ratio<u64>,
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
    /// Block completion announcement.
    #[from]
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::StorageRequest(req) => req.fmt(f),
            Event::NetRequestIncoming(incoming) => incoming.fmt(f),
            Event::StateStoreRequest(req) => req.fmt(f),
            Event::MarkBlockCompletedRequest(req) => req.fmt(f),
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

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let result = match event {
            Event::StorageRequest(req) => self.handle_storage_request::<REv>(req),
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
            Event::StateStoreRequest(req) => {
                self.handle_state_store_request::<REv>(effect_builder, req)
            }
            Event::MarkBlockCompletedRequest(req) => self.handle_mark_block_completed_request(req),
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: &WithDir<Config>,
        fault_tolerance_fraction: Ratio<u64>,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
        network_name: &str,
        recent_era_count: u64,
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
        for (_, raw_val) in cursor.iter() {
            let mut body_txn = env.begin_ro_txn()?;
            let block_header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
            let maybe_block_body =
                get_body_for_block_header(&mut body_txn, &block_header, block_body_db);
            if let Some(invalid_era) = hard_reset_to_start_of_era {
                // Remove blocks that are in to-be-upgraded eras, but have obsolete protocol
                // versions - they were most likely created before the upgrade and should be
                // reverted.
                if block_header.era_id() >= invalid_era
                    && block_header.protocol_version() < protocol_version
                {
                    let _ = deleted_block_hashes.insert(block_header.hash());

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
                    block_header.hash(),
                    &block_body,
                    block_header.height(),
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

        let mut component = Self {
            root,
            env,
            block_header_db,
            block_body_db,
            block_metadata_db,
            deploy_db,
            deploy_metadata_db,
            transfer_db,
            state_store_db,
            finalized_approvals_db,
            block_height_index,
            switch_block_era_id_index,
            deploy_hash_index,
            completed_blocks: Default::default(),
            enable_mem_deduplication: config.enable_mem_deduplication,
            serialized_item_pool: ObjectPool::new(config.mem_pool_prune_interval),
            recent_era_count,
            fault_tolerance_fraction,
        };

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
                //    have the corresponding global state in contract runtime, due to the way sync
                //    worked previously.
                // 2. Any block acquired from that point onwards was subject to the insertion of the
                //    appropriate announcements (`BlockCompletedAnnouncement`), which would have
                //    caused the creation of the completed blocks index, thus would not have
                //    resulted in a `None` value here.
                //
                // For this reason, it is safe to assume that, when handling the `None` case, all
                // blocks in storage are complete blocks.
                if let Some(&highest_block) = component.block_height_index.keys().last() {
                    component.completed_blocks =
                        DisjointSequences::new(Sequence::new(0, highest_block));
                    component.persist_completed_blocks()?;
                } // the `else` case here would mean genesis, so no change.
            }
        }

        Ok(component)
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
                self.write_state_store(key, &data)?;
                Ok(responder.respond(()).ignore())
            }
            StateStoreRequest::Load { key, responder } => {
                let bytes = self.read_state_store(&key)?;
                Ok(responder.respond(bytes).ignore())
            }
        }
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

        match incoming.message {
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
            NetRequest::BlockDeployApprovals(ref serialized_id) => {
                let id = decode_item_id::<BlockDeployApprovals>(serialized_id)?;

                let opt_item = self
                    .read_block_and_deploys_by_hash(id)
                    .map_err(FatalStorageError::from)?
                    .map(|block_and_deploys| {
                        // TODO: Get the _finalized_ approvals.
                        BlockDeployApprovals::new(
                            id,
                            block_and_deploys
                                .deploys
                                .into_iter()
                                .map(|deploy| {
                                    (
                                        *deploy.id(),
                                        FinalizedApprovals::new(deploy.approvals().clone()),
                                    )
                                })
                                .collect(),
                        )
                    });
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
            NetRequest::GossipedAddress(_) => Err(GetRequestError::GossipedAddressNotGettable),
            NetRequest::BlockAndMetadataByHeight(ref serialized_id) => {
                let item_id = decode_item_id::<BlockWithMetadata>(serialized_id)?;
                let opt_item = self
                    .read_block_and_metadata_by_height(item_id)
                    .map_err(FatalStorageError::from)?;
                let fetch_response = FetchResponse::from_opt(item_id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::BlockHeaderByHash(ref serialized_id) => {
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
            NetRequest::BlockHeaderAndFinalitySignaturesByHeight(ref serialized_id) => {
                let item_id = decode_item_id::<BlockHeaderWithMetadata>(serialized_id)?;
                let opt_item = self
                    .read_block_header_and_metadata_by_height(item_id)
                    .map_err(FatalStorageError::from)?;

                let fetch_response = match opt_item {
                    Some(item) => {
                        let highest_header = {
                            let mut txn =
                                self.env.begin_ro_txn().map_err(FatalStorageError::from)?;
                            self.get_highest_block_header(&mut txn)?.ok_or_else(|| {
                                error!("should have highest header as at least one header stored");
                                GetRequestError::FailedToGetHighestBlockHeader
                            })?
                        };
                        if item
                            .block_header
                            .era_id()
                            .saturating_add(self.recent_era_count)
                            > highest_header.era_id()
                        {
                            FetchResponse::Fetched(item)
                        } else {
                            FetchResponse::NotProvided(item_id)
                        }
                    }
                    None => FetchResponse::NotFound(item_id),
                };

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::BlockAndDeploys(ref serialized_id) => {
                let item_id = decode_item_id::<BlockAndDeploys>(serialized_id)?;
                let opt_item = self
                    .read_block_and_deploys_by_hash(item_id)
                    .map_err(FatalStorageError::from)?;
                let fetch_response = FetchResponse::from_opt(item_id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::BlockHeadersBatch(ref serialized_id) => {
                let item_id = decode_item_id::<BlockHeadersBatch>(serialized_id)?;
                let opt_item = self.read_block_headers_batch(&item_id)?;
                let fetch_response = FetchResponse::from_opt(item_id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::FinalitySignatures(ref serialized_id) => {
                let item_id = decode_item_id::<BlockSignatures>(serialized_id)?;
                let opt_item = self.read_block_signatures(&item_id)?;
                let fetch_response = FetchResponse::from_opt(item_id, opt_item);

                Ok(self.update_pool_and_send(
                    effect_builder,
                    incoming.sender,
                    serialized_id,
                    fetch_response,
                )?)
            }
            NetRequest::SyncLeap(ref serialized_id) => {
                let item_id = decode_item_id::<SyncLeap>(serialized_id)?;
                let fetch_response = self.get_sync_leap(item_id, self.recent_era_count)?;

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
                responder.respond(self.write_block(&*block)?).ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => responder.respond(self.read_block(&block_hash)?).ignore(),
            StorageRequest::GetHighestBlock { responder } => {
                responder.respond(self.read_highest_block()?).ignore()
            }
            StorageRequest::GetHighestBlockHeader { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_highest_block_header(&mut txn)?)
                    .ignore()
            }
            StorageRequest::GetSwitchBlockHeaderAtEraId { era_id, responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_switch_block_header_by_era_id(&mut txn, era_id)?)
                    .ignore()
            }
            StorageRequest::GetBlockHeaderForDeploy {
                deploy_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_block_header_by_deploy_hash(&mut txn, deploy_hash)?)
                    .ignore()
            }
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
            StorageRequest::CheckBlockHeaderExistence {
                block_height,
                responder,
            } => responder
                .respond(self.block_header_exists(block_height))
                .ignore(),
            StorageRequest::GetBlockTransfers {
                block_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_transfers(&mut txn, &block_hash)?)
                    .ignore()
            }
            StorageRequest::PutDeploy { deploy, responder } => {
                responder.respond(self.put_deploy(&*deploy)?).ignore()
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
            StorageRequest::GetBlockHeaderAndMetadataByHash {
                block_hash,
                only_from_available_block_range,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;

                let block_header: BlockHeader = {
                    if let Some(block_header) = self.get_single_block_header_restricted(
                        &mut txn,
                        &block_hash,
                        only_from_available_block_range,
                    )? {
                        block_header
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
                };
                let block_signatures = match self.get_block_signatures(&mut txn, &block_hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(block_hash, block_header.era_id()),
                };
                responder
                    .respond(Some(BlockHeaderWithMetadata {
                        block_header,
                        block_signatures,
                    }))
                    .ignore()
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
            StorageRequest::GetBlockHeaderAndMetadataByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            } => {
                if !(self.should_return_block(block_height, only_from_available_block_range)?) {
                    return Ok(responder.respond(None).ignore());
                }

                let mut txn = self.env.begin_ro_txn()?;

                let block_header = {
                    if let Some(block_header) =
                        self.get_block_header_by_height(&mut txn, block_height)?
                    {
                        block_header
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
                };

                let hash = block_header.hash();
                let block_signatures = match self.get_block_signatures(&mut txn, &hash)? {
                    Some(signatures) => signatures,
                    None => BlockSignatures::new(hash, block_header.era_id()),
                };
                responder
                    .respond(Some(BlockHeaderWithMetadata {
                        block_header,
                        block_signatures,
                    }))
                    .ignore()
            }
            StorageRequest::GetHighestBlockWithMetadata { responder } => {
                let mut txn = self.env.begin_ro_txn()?;

                let highest_block: Block = {
                    if let Some(block) = self
                        .block_height_index
                        .keys()
                        .last()
                        .and_then(|&height| self.get_block_by_height(&mut txn, height).transpose())
                        .transpose()?
                    {
                        block
                    } else {
                        return Ok(responder.respond(None).ignore());
                    }
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
            StorageRequest::GetBlockSignatures {
                block_hash,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_block_signatures(&mut txn, &block_hash)?)
                    .ignore()
            }
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
            StorageRequest::GetFinalizedBlocks { ttl, responder } => {
                responder.respond(self.get_finalized_blocks(ttl)?).ignore()
            }
            StorageRequest::GetBlockHeaderByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            } => {
                let mut txn = self.env.begin_ro_txn()?;
                let result = self.get_block_header_by_height_restricted(
                    &mut txn,
                    block_height,
                    only_from_available_block_range,
                )?;
                responder.respond(result).ignore()
            }
            StorageRequest::PutBlockHeader {
                block_header,
                responder,
            } => {
                let block_header_hash = block_header.hash();
                match self.put_block_headers(vec![*block_header]) {
                    Ok(result) => responder.respond(result).ignore(),
                    Err(err) => {
                        error!(?err, ?block_header_hash, "error when storing block header");
                        return Err(err);
                    }
                }
            }
            StorageRequest::PutHeadersBatch {
                block_headers,
                responder,
            } => responder
                .respond(self.put_block_headers(block_headers)?)
                .ignore(),
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
            StorageRequest::PutBlockAndDeploys { block, responder } => responder
                .respond(self.put_block_and_deploys(&*block)?)
                .ignore(),
            StorageRequest::GetBlockAndDeploys {
                block_hash,
                responder,
            } => responder
                .respond(self.read_block_and_deploys_by_hash(block_hash)?)
                .ignore(),
            StorageRequest::GetHeadersBatch {
                block_headers_id,
                responder,
            } => responder
                .respond(self.read_block_headers_batch(&block_headers_id)?)
                .ignore(),
        })
    }

    /// Handles a [`BlockCompletedAnnouncement`].
    fn handle_mark_block_completed_request(
        &mut self,
        MarkBlockCompletedRequest {
            block_height,
            responder,
        }: MarkBlockCompletedRequest,
    ) -> Result<Effects<Event>, FatalStorageError> {
        self.completed_blocks.insert(block_height);
        self.persist_completed_blocks()?;

        Ok(responder.respond(()).ignore())
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
        let outcome = txn.put_value(self.deploy_db, deploy.id(), deploy, false)?;
        txn.commit()?;
        Ok(outcome)
    }

    /// Puts block and its deploys into storage.
    ///
    /// Returns `Ok` only if the block and all deploys were successfully written.
    fn put_block_and_deploys(
        &mut self,
        block_and_deploys: &BlockAndDeploys,
    ) -> Result<(), FatalStorageError> {
        let BlockAndDeploys { block, deploys } = block_and_deploys;

        block.verify()?;
        let deploy_db = self.deploy_db;
        let (wrote, mut txn) = self.write_validated_block(block)?;
        if !wrote {
            return Err(FatalStorageError::FailedToOverwriteBlock);
        }

        for deploy in deploys {
            let _ = txn.put_value(deploy_db, deploy.id(), deploy, false)?;
        }
        txn.commit()?;

        Ok(())
    }

    /// Retrieves a block by hash.
    pub fn read_block(&self, block_hash: &BlockHash) -> Result<Option<Block>, FatalStorageError> {
        self.get_single_block(&mut self.env.begin_ro_txn()?, block_hash)
    }

    /// Gets the highest block.
    pub fn read_highest_block(&self) -> Result<Option<Block>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.get_highest_block(&mut txn)
    }

    /// Writes a block to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    pub fn write_block(&mut self, block: &Block) -> Result<bool, FatalStorageError> {
        // Validate the block prior to inserting it into the database
        block.verify()?;
        let (wrote, txn) = self.write_validated_block(block)?;
        if wrote {
            txn.commit()?;
        }
        Ok(wrote)
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
        block: &Block,
    ) -> Result<(bool, RwTransaction), FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        {
            let block_body_hash = block.header().body_hash();
            let block_body = block.body();
            if !self.put_single_block_body(&mut txn, block_body_hash, block_body)? {
                error!("Could not insert body for: {}", block);
                return Ok((false, txn));
            }
        }

        let overwrite = true;

        if !txn.put_value(
            self.block_header_db,
            block.hash(),
            block.header(),
            overwrite,
        )? {
            error!("Could not insert block header for block: {}", block);
            return Ok((false, txn));
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
            )?;
        }
        Ok((true, txn))
    }

    /// Get the switch block header for a specified [`EraID`].
    pub(crate) fn read_switch_block_header_by_era_id(
        &self,
        switch_block_era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.get_switch_block_header_by_era_id(&mut txn, switch_block_era_id)
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

    /// Retrieves a block header by height, together with all stored block signatures.
    ///
    /// Returns `None` if the block header is not stored, or if no block signatures are stored for
    /// it.
    pub fn read_block_header_and_metadata_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");
        let block_header =
            if let Some(block_header) = self.get_block_header_by_height(&mut txn, height)? {
                block_header
            } else {
                return Ok(None);
            };
        let block_signatures = if let Some(block_signatures) =
            self.get_block_signatures(&mut txn, &block_header.hash())?
        {
            block_signatures
        } else {
            debug!(height, "no block signatures stored for block header");
            return Ok(None);
        };
        Ok(Some(BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        }))
    }

    /// Retrieves single block and all of its deploys.
    /// If any of the deploys can't be found, returns `Ok(None)`.
    fn read_block_and_deploys_by_hash(
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
        txn: &mut Tx,
        height: u64,
    ) -> Result<Option<Block>, FatalStorageError> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block(txn, block_hash).transpose())
            .transpose()
    }

    /// Retrieves single block header by height by looking it up in the index and returning it.
    fn get_block_header_by_height<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        height: u64,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| {
                self.get_single_block_header_restricted(txn, block_hash, false)
                    .transpose()
            })
            .transpose()
    }

    /// Retrieves single switch block header by era ID by looking it up in the index and returning
    /// it.
    fn get_switch_block_header_by_era_id<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        debug!(switch_block_era_id_index = ?self.switch_block_era_id_index);
        self.switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| self.get_single_block_header(txn, block_hash).transpose())
            .transpose()
    }

    /// Retrieves a single block header by deploy hash by looking it up in the index and returning
    /// it.
    fn get_block_header_by_deploy_hash<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        deploy_hash: DeployHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.deploy_hash_index
            .get(&deploy_hash)
            .and_then(|block_hash_and_height| {
                self.get_single_block_header(txn, &block_hash_and_height.block_hash)
                    .transpose()
            })
            .transpose()
    }

    /// Retrieves the block hash and height for a deploy hash by looking it up in the index
    /// and returning it.
    fn get_block_hash_and_height_by_deploy_hash(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<BlockHashAndHeight>, FatalStorageError> {
        Ok(self.deploy_hash_index.get(&deploy_hash).copied())
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

    /// Retrieves the highest block header from storage, if one exists. May return an LMDB error.
    fn get_highest_block_header<Tx: Transaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.block_height_index
            .iter()
            .last()
            .and_then(|(_, hash_of_highest_block)| {
                self.get_single_block_header(txn, hash_of_highest_block)
                    .transpose()
            })
            .transpose()
    }

    /// Retrieves the highest block header from storage, if one exists. May return an LMDB error.
    fn get_header_of_highest_complete_block<Tx: Transaction>(
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

    /// Returns the vector of blocks that could still have deploys whose TTL hasn't expired yet.
    fn get_finalized_blocks(&self, ttl: TimeDiff) -> Result<Vec<Block>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        // We're interested in deploys whose TTL hasn't expired yet.
        let ttl_not_expired = |block: &Block| block.timestamp().elapsed() < ttl;
        self.get_blocks_while(&mut txn, ttl_not_expired)
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

        self.validate_block_header_hash(&block_header, block_hash)?;
        Ok(Some(block_header))
    }

    /// Returns block headers of the trusted block's ancestors, back to the most recent switch
    /// block. If the trusted one is already a switch block, returns empty vec.
    fn get_trusted_ancestor_headers<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        trusted_block_header: &BlockHeader,
    ) -> Result<Option<Vec<BlockHeader>>, FatalStorageError> {
        if trusted_block_header.is_switch_block() {
            return Ok(Some(vec![]));
        }

        let mut current_trusted_block_header = trusted_block_header.clone();

        let mut result = vec![];
        loop {
            let parent_hash = current_trusted_block_header.parent_hash();
            let parent_block_header: BlockHeader =
                match txn.get_value(self.block_header_db, &parent_hash)? {
                    Some(block_header) => block_header,
                    None => {
                        warn!(?parent_hash, "block header not found");
                        return Ok(None);
                    }
                };

            result.push(parent_block_header.clone());
            if parent_block_header.is_switch_block() {
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
        mut switch_block: BlockHeader,
    ) -> Result<Option<Vec<BlockHeaderWithMetadata>>, FatalStorageError> {
        if trusted_block_header.hash() == highest_signed_block_header.block_header.hash() {
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

            let block = match self.get_single_block_header_with_metadata(txn, hash)? {
                Some(block) => block,
                None => return Ok(None),
            };

            let next_era_validator_weights = match switch_block.next_era_validator_weights() {
                Some(next_era_validator_weights) => next_era_validator_weights,
                None => return Ok(None),
            };

            if linear_chain::check_sufficient_block_signatures(
                next_era_validator_weights,
                self.fault_tolerance_fraction,
                Some(&block.block_signatures),
            )
            .is_ok()
            {
                switch_block = block.block_header.clone();
                result.push(block);
            } else {
                return Ok(None);
            }
        }
        result.push(highest_signed_block_header.clone());

        Ok(Some(result))
    }

    fn get_block_header_by_height_restricted<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_height: u64,
        only_from_available_block_range: bool,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_hash = match self.block_height_index.get(&block_height) {
            None => return Ok(None),
            Some(block_hash) => block_hash,
        };

        self.get_single_block_header_restricted(txn, block_hash, only_from_available_block_range)
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
        self.validate_block_header_hash(&block_header, block_hash)?;
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
        let block_header_hash = block_header.hash();

        let block_signatures = match self.get_block_signatures(txn, &block_header_hash)? {
            Some(signatures) => signatures,
            None => BlockSignatures::new(block_header_hash, block_header.era_id()),
        };

        Ok(Some(BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        }))
    }

    /// Validates the block header hash against the expected block hash.
    fn validate_block_header_hash(
        &self,
        block_header: &BlockHeader,
        block_hash: &BlockHash,
    ) -> Result<(), FatalStorageError> {
        let found_block_header_hash = block_header.hash();
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
    fn block_header_exists(&self, block_height: u64) -> bool {
        self.block_height_index.contains_key(&block_height)
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
            let block_header_hash = block_header.hash();
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
            None => return Ok(None),
        };
        let maybe_block_body = get_body_for_block_header(txn, &block_header, self.block_body_db);
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
        deploy_hash: DeployHash,
    ) -> Result<Option<Deploy>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        Ok(txn.get_value(self.deploy_db, &deploy_hash)?)
    }

    /// Directly returns all deploys or None if any is missing.
    fn read_deploys<'a, I: Iterator<Item = &'a DeployHash> + 'a>(
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

    /// Retrieves a deploy from the deploy store.
    fn get_deploy(&self, deploy_hash: DeployHash) -> Result<Option<Deploy>, LmdbExtError> {
        self.env
            .begin_ro_txn()
            .map_err(Into::into)
            .and_then(|mut txn| txn.get_value(self.deploy_db, &deploy_hash))
    }

    fn read_block_headers_batch(
        &self,
        block_header_ids: &BlockHeadersBatchId,
    ) -> Result<Option<BlockHeadersBatch>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;

        let mut headers = Vec::with_capacity(block_header_ids.len() as usize);
        for block_height in block_header_ids.iter() {
            match self.get_block_header_by_height_restricted(&mut txn, block_height, true)? {
                Some(block_header) => headers.push(block_header),
                None => {
                    debug!(?block_height, "block header not found");
                    // Short-circuit, we're all interested in the complete data.
                    return Ok(None);
                }
            }
        }
        Ok(BlockHeadersBatch::from_vec(headers, block_header_ids))
    }

    pub(crate) fn get_sync_leap(
        &self,
        block_hash: BlockHash,
        allowed_era_diff: u64,
    ) -> Result<FetchResponse<SyncLeap, BlockHash>, FatalStorageError> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("could not create RO transaction");

        let trusted_block_header = match self.get_single_block_header(&mut txn, &block_hash)? {
            Some(trusted_block_header) => trusted_block_header,
            None => return Ok(FetchResponse::NotFound(block_hash)),
        };

        let highest_complete_block_header =
            match self.get_header_of_highest_complete_block(&mut txn)? {
                Some(highest_complete_block_header) => highest_complete_block_header,
                None => return Ok(FetchResponse::NotFound(block_hash)),
            };
        if highest_complete_block_header
            .block_header
            .era_id()
            .saturating_sub(trusted_block_header.era_id().into())
            > allowed_era_diff.into()
        {
            return Ok(FetchResponse::NotProvided(block_hash));
        }

        let trusted_ancestor_headers =
            match self.get_trusted_ancestor_headers(&mut txn, &trusted_block_header)? {
                Some(trusted_ancestor_headers) => trusted_ancestor_headers,
                None => return Ok(FetchResponse::NotFound(block_hash)),
            };

        if let Some(signed_block_headers) = self.get_signed_block_headers(
            &mut txn,
            &trusted_block_header,
            &highest_complete_block_header,
            trusted_ancestor_headers
                .last()
                .cloned()
                .unwrap_or_else(|| trusted_block_header.clone()),
        )? {
            Ok(FetchResponse::Fetched(SyncLeap {
                trusted_block_header,
                trusted_ancestor_headers,
                signed_block_headers,
            }))
        } else {
            Ok(FetchResponse::NotFound(block_hash))
        }
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
        T: FetcherItem,
    {
        let serialized = fetch_response
            .to_serialized()
            .map_err(FatalStorageError::StoredItemSerializationFailure)?;
        let shared: Arc<[u8]> = serialized.into();

        if self.enable_mem_deduplication && fetch_response.was_found() {
            self.serialized_item_pool
                .put(serialized_id.into(), Arc::downgrade(&shared));
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
            Ok(self.get_available_block_range().contains(block_height))
        } else {
            Ok(true)
        }
    }

    fn get_available_block_range(&self) -> AvailableBlockRange {
        match self.completed_blocks.highest_sequence() {
            Some(&seq) => seq.into(),
            None => AvailableBlockRange::RANGE_0_0,
        }
    }
}

/// Decodes an item's ID, typically from an incoming request.
fn decode_item_id<T>(raw: &[u8]) -> Result<T::Id, GetRequestError>
where
    T: FetcherItem,
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
    let block_hash = block_header.hash();
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

    let _ = block_height_index.insert(block_header.height(), block_hash);
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
    pub(crate) fn transactional_get_switch_block_by_era_id(
        &self,
        switch_block_era_num: u64,
    ) -> Result<Option<Block>, FatalStorageError> {
        let mut txn = self
            .env
            .begin_ro_txn()
            .expect("Could not start read only transaction for lmdb");
        let switch_block = self
            .get_switch_block_by_era_id(&mut txn, EraId::from(switch_block_era_num))
            .expect("LMDB panicked trying to get switch block");
        txn.commit().expect("Could not commit transaction");
        Ok(switch_block)
    }

    /// Retrieves a single block header by height by looking it up in the index and returning it.
    pub fn read_block_header_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| {
                self.get_single_block_header(&mut txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    /// Retrieves the highest block header from the storage, if one exists.
    pub fn read_highest_block_header(&self) -> Result<Option<BlockHeader>, FatalStorageError> {
        let highest_block_hash = match self.block_height_index.iter().last() {
            Some((_, highest_block_hash)) => highest_block_hash,
            None => return Ok(None),
        };
        self.read_block_header_by_hash(highest_block_hash)
    }
}

fn construct_block_body_to_block_header_reverse_lookup(
    txn: &impl Transaction,
    block_header_db: &Database,
) -> Result<BTreeMap<Digest, BlockHeader>, LmdbExtError> {
    let mut block_body_hash_to_header_map: BTreeMap<Digest, BlockHeader> = BTreeMap::new();
    for (_raw_key, raw_val) in txn.open_ro_cursor(*block_header_db)?.iter() {
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

    for (raw_key, _raw_val) in cursor.iter() {
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
    block_header: &BlockHeader,
    block_body_db: Database,
) -> Result<Option<BlockBody>, LmdbExtError> {
    txn.get_value(block_body_db, block_header.body_hash())
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
