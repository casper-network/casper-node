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
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use static_assertions::const_assert;
#[cfg(test)]
use tempfile::TempDir;
use thiserror::Error;
use tracing::{debug, error, info, warn};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    EraId, ExecutionResult, ProtocolVersion, PublicKey, Transfer, Transform,
};

// The reactor! macro needs this in the fetcher tests
pub(crate) use crate::effect::requests::StorageRequest;

use crate::{
    components::{
        consensus, consensus::error::FinalitySignatureError, fetcher::FetchedOrNotFound, Component,
    },
    crypto,
    effect::{
        incoming::{NetRequest, NetRequestIncoming},
        requests::{NetworkRequest, StateStoreRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    protocol::Message,
    reactor::ReactorEvent,
    types::{
        error::BlockValidationError, Block, BlockBody, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockSignatures, BlockWithMetadata, Deploy, DeployHash,
        DeployMetadata, HashingAlgorithmVersion, Item, MerkleBlockBody, MerkleBlockBodyPart,
        MerkleLinkedListNode, NodeId, TimeDiff,
    },
    utils::{display_error, WithDir},
    NodeRng,
};
use lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt};
use object_pool::ObjectPool;

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

/// A fatal storage component error.
///
/// An error of this kinds indicates that storage is corrupted or otherwise irrecoverably broken, at
/// least for the moment. It should usually be followed by swift termination of the node.
#[derive(Debug, Error)]
pub enum FatalStorageError {
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
    /// Switch block does not contain era end.
    #[error("switch block does not contain era end: {0:?}")]
    InvalidSwitchBlock(Box<BlockHeader>),
    /// Insufficient or wrong finality signatures.
    #[error(transparent)]
    FinalitySignature(#[from] FinalitySignatureError),
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
    /// Failed to serialize an item that was found in local storage.
    #[error("failed to serialized stored item")]
    StoredItemSerializationFailure(#[source] bincode::Error),
}

// We wholesale wrap lmdb errors and treat them as internal errors here.
impl From<lmdb::Error> for FatalStorageError {
    fn from(err: lmdb::Error) -> Self {
        LmdbExtError::from(err).into()
    }
}

/// An error that may occur when handling a get request.
///
/// Wraps a fatal error, callers should check whether the variant is of the fatal or non-fatal kind.
#[derive(Debug, Error)]
enum GetRequestError {
    /// A fatal error occurred.
    #[error(transparent)]
    Fatal(#[from] FatalStorageError),
    /// Failed to serialized an item ID on an incoming item request.
    #[error("failed to deserialize incoming item id")]
    MalformedIncomingItemId(#[source] bincode::Error),
    /// Received a get request for a gossiped address, which is unanswerable.
    #[error("received a request for a gossiped address")]
    GossipedAddressNotGettable,
}

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
    /// An in-memory pool of already loaded serialized items.
    ///
    /// Keyed by serialized item ID, contains the serialized item.
    serialized_item_pool: ObjectPool<Box<[u8]>>,
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
        let outcome = match event {
            Event::StorageRequest(req) => self.handle_storage_request::<REv>(req),
            Event::NetRequestIncoming(ref incoming) => {
                match self.handle_net_request_incoming::<REv>(effect_builder, incoming) {
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
            Event::StateStoreRequest(req) => {
                self.handle_state_store_request::<REv>(effect_builder, req)
            }
        };

        // On success, execute the effects. For errors, we crash on fatal ones, but only log
        // non-fatal ones.
        match outcome {
            Ok(effects) => effects,
            Err(ref err) => {
                // Any error is turned into a fatal effect, the component itself does not panic.
                fatal!(effect_builder, "storage error: {}", display_error(err)).ignore()
            }
        }
    }
}

impl Storage {
    /// Creates a new storage component.
    ///
    /// If `should_check_integrity` is true, time-consuming integrity checks will be performed
    /// during this call to `new()`, potentially blocking for several minutes.  This should normally
    /// only be required if the node is detected to have restarted after a crash.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: &WithDir<Config>,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
        should_check_integrity: bool,
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
                    if block_header.hashing_algorithm_version(verifiable_chunked_hash_activation)
                        == HashingAlgorithmVersion::V1
                    {
                        let _ = deleted_block_body_hashes_v1.insert(*block_header.body_hash());
                    }
                    let _ = deleted_block_hashes
                        .insert(block_header.hash(verifiable_chunked_hash_activation));
                    cursor.del(WriteFlags::empty())?;
                    continue;
                }
            }

            if should_check_integrity {
                let found_block_header_hash = block_header.hash(verifiable_chunked_hash_activation);
                if raw_key != found_block_header_hash.as_ref() {
                    return Err(FatalStorageError::BlockHeaderNotStoredUnderItsHash {
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
                verifiable_chunked_hash_activation,
            )?;

            let mut body_txn = env.begin_ro_txn()?;
            let maybe_block_body =
                match block_header.hashing_algorithm_version(verifiable_chunked_hash_activation) {
                    HashingAlgorithmVersion::V1 => {
                        body_txn.get_value(block_body_v1_db, block_header.body_hash())?
                    }
                    HashingAlgorithmVersion::V2 => get_single_block_body_v2(
                        &mut body_txn,
                        block_body_v2_db,
                        deploy_hashes_db,
                        transfer_hashes_db,
                        proposer_db,
                        block_header.body_hash(),
                    )?,
                };

            if let Some(block_body) = maybe_block_body {
                if should_check_integrity {
                    Block::new_from_header_and_body(
                        block_header.clone(),
                        block_body.clone(),
                        verifiable_chunked_hash_activation,
                    )?;
                }

                insert_to_deploy_index(
                    &mut deploy_hash_index,
                    block_header.hash(verifiable_chunked_hash_activation),
                    &block_body,
                )?;
            }
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
            verifiable_chunked_hash_activation,
        )?;
        initialize_block_body_v2_db(
            &env,
            &block_header_db,
            &block_body_v2_db,
            &deploy_hashes_db,
            &transfer_hashes_db,
            &proposer_db,
            should_check_integrity,
            verifiable_chunked_hash_activation,
        )?;
        initialize_block_metadata_db(
            &env,
            &block_metadata_db,
            &deleted_block_hashes_raw,
            should_check_integrity,
        )?;
        initialize_deploy_metadata_db(&env, &deploy_metadata_db, &deleted_block_hashes)?;

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
            block_height_index,
            switch_block_era_id_index,
            deploy_hash_index,
            enable_mem_deduplication: config.enable_mem_deduplication,
            serialized_item_pool: ObjectPool::new(config.mem_pool_prune_interval),
            finality_threshold_fraction,
            last_emergency_restart,
            verifiable_chunked_hash_activation,
        })
    }

    /// Handles a state store request.
    fn handle_state_store_request<REv>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        req: StateStoreRequest,
    ) -> Result<Effects<Event>, FatalStorageError>
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
        &mut self,
        effect_builder: EffectBuilder<REv>,
        incoming: &NetRequestIncoming,
    ) -> Result<Effects<Event>, GetRequestError>
    where
        Self: Component<REv>,
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
        }
    }

    /// Handles a storage request.
    fn handle_storage_request<REv>(
        &mut self,
        req: StorageRequest,
    ) -> Result<Effects<Event>, FatalStorageError>
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
            StorageRequest::GetHighestBlock { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_highest_block(&mut txn)?)
                    .ignore()
            }
            StorageRequest::GetHighestBlockHeader { responder } => {
                let mut txn = self.env.begin_ro_txn()?;
                responder
                    .respond(self.get_highest_block_header(&mut txn)?)
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
            StorageRequest::CheckBlockHeaderExistence {
                block_height,
                responder,
            } => responder
                .respond(self.block_header_exists(block_height))
                .ignore(),
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
            StorageRequest::GetBlockHeaderAndSufficientFinalitySignaturesByHeight {
                block_height,
                responder,
            } => {
                let result = self.get_block_header_and_sufficient_finality_signatures_by_height(
                    &mut self.env.begin_ro_txn()?,
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
                insert_to_block_header_indices(
                    &mut self.block_height_index,
                    &mut self.switch_block_era_id_index,
                    &block_header,
                    self.verifiable_chunked_hash_activation,
                )?;
                txn.commit()?;
                responder.respond(true).ignore()
            }
        })
    }

    /// Put a single deploy into storage.
    pub fn put_deploy(&self, deploy: &Deploy) -> Result<bool, FatalStorageError> {
        let mut txn = self.env.begin_rw_txn()?;
        let outcome = txn.put_value(self.deploy_db, deploy.id(), &deploy, false)?;
        txn.commit()?;
        Ok(outcome)
    }

    /// Retrieves a block by hash.
    pub fn read_block(&self, block_hash: &BlockHash) -> Result<Option<Block>, FatalStorageError> {
        self.get_single_block(&mut self.env.begin_ro_txn()?, block_hash)
    }

    /// Directly returns a deploy from internal store.
    pub fn read_deploy_by_hash(
        &self,
        deploy_hash: DeployHash,
    ) -> Result<Option<Deploy>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        Ok(txn.get_value(self.deploy_db, &deploy_hash)?)
    }

    /// Writes a block to storage, updating indices as necessary
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    pub fn write_block(&mut self, block: &Block) -> Result<bool, FatalStorageError> {
        // Validate the block prior to inserting it into the database
        block.verify(self.verifiable_chunked_hash_activation)?;
        let mut txn = self.env.begin_rw_txn()?;
        // Write the block body
        {
            let block_body_hash = block.header().body_hash();
            let block_body = block.body();
            let success = match block
                .header()
                .hashing_algorithm_version(self.verifiable_chunked_hash_activation)
            {
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
            self.verifiable_chunked_hash_activation,
        )?;
        insert_to_deploy_index(
            &mut self.deploy_hash_index,
            block.header().hash(self.verifiable_chunked_hash_activation),
            block.body(),
        )?;
        txn.commit()?;
        Ok(true)
    }

    /// Get the switch block header for a specified [`EraID`].
    pub(crate) fn read_switch_block_header_by_era_id(
        &self,
        switch_block_era_id: EraId,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.get_switch_block_header_by_era_id(&mut self.env.begin_ro_txn()?, switch_block_era_id)
    }

    /// Retrieves a block header by height.
    /// Returns `None` if they are less than the fault tolerance threshold, or if the block is from
    /// before the most recent emergency upgrade.
    pub fn read_block_header_and_sufficient_finality_signatures_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let mut txn = self.env.begin_ro_txn()?;
        let maybe_block_header_and_finality_signatures =
            self.get_block_header_and_sufficient_finality_signatures_by_height(&mut txn, height)?;
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
        let maybe_block_and_finality_signatures =
            self.get_block_and_sufficient_finality_signatures_by_height(&mut txn, height)?;
        drop(txn);
        Ok(maybe_block_and_finality_signatures)
    }

    /// Retrieves single block header by height by looking it up in the index and returning it.
    fn get_block_header_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        height: u64,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.block_height_index
            .get(&height)
            .and_then(|block_hash| self.get_single_block_header(tx, block_hash).transpose())
            .transpose()
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    pub fn read_block_by_height(&self, height: u64) -> Result<Option<Block>, FatalStorageError> {
        self.get_block_by_height(&mut self.env.begin_ro_txn()?, height)
    }

    /// Gets the highest block.
    pub fn read_highest_block(&self) -> Result<Option<Block>, FatalStorageError> {
        self.get_highest_block(&mut self.env.begin_ro_txn()?)
    }

    /// Retrieves a block header to handle a network request.
    pub fn read_block_header_by_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.get_block_header_by_height(&mut self.env.begin_ro_txn()?, height)
    }

    /// Retrieves single block by height by looking it up in the index and returning it.
    fn get_block_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        height: u64,
    ) -> Result<Option<Block>, FatalStorageError> {
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
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
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
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        self.deploy_hash_index
            .get(&deploy_hash)
            .and_then(|block_hash| self.get_single_block_header(tx, block_hash).transpose())
            .transpose()
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

    /// Retrieves the highest block header from the storage, if one exists.
    pub fn read_highest_block_header(&self) -> Result<Option<BlockHeader>, FatalStorageError> {
        let highest_block_hash = match self.block_height_index.iter().last() {
            Some((_, highest_block_hash)) => highest_block_hash,
            None => return Ok(None),
        };
        self.read_block_header_by_hash(highest_block_hash)
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

    /// Retrieves the state root hashes from storage to check the integrity of the trie store.
    pub(crate) fn read_state_root_hashes_for_trie_check(
        &self,
    ) -> Result<HashSet<Digest>, FatalStorageError> {
        let mut hashes = HashSet::new();
        let txn = self.env.begin_ro_txn()?;
        let mut cursor = txn.open_ro_cursor(self.block_header_db)?;
        for (_, raw_val) in cursor.iter() {
            let header: BlockHeader = lmdb_ext::deserialize(raw_val)?;
            let hash = *header.state_root_hash();
            hashes.insert(hash);
        }

        Ok(hashes)
    }

    /// Retrieves a single block header in a separate transaction from storage.
    fn get_single_block_header<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, FatalStorageError> {
        let block_header: BlockHeader = match tx.get_value(self.block_header_db, &block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let found_block_header_hash = block_header.hash(self.verifiable_chunked_hash_activation);
        if found_block_header_hash != *block_hash {
            return Err(FatalStorageError::BlockHeaderNotStoredUnderItsHash {
                queried_block_hash_bytes: block_hash.as_ref().to_vec(),
                found_block_header_hash,
                block_header: Box::new(block_header),
            });
        };
        Ok(Some(block_header))
    }

    /// Checks whether a block at the given height exists in the block height index (and, since the
    /// index is initialized on startup based on the actual contents of storage, if it exists in
    /// storage).
    fn block_header_exists(&self, block_height: u64) -> bool {
        self.block_height_index.contains_key(&block_height)
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
        let maybe_block_body = self.get_body_for_block_header(tx, &block_header)?;
        let block_body = match maybe_block_body {
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

    fn get_body_for_block_header<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        block_header: &BlockHeader,
    ) -> Result<Option<BlockBody>, LmdbExtError> {
        match block_header.hashing_algorithm_version(self.verifiable_chunked_hash_activation) {
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
        height: u64,
    ) -> Result<Option<BlockWithMetadata>, FatalStorageError> {
        let BlockHeaderWithMetadata {
            block_header,
            block_signatures,
        } = match self.get_block_header_and_sufficient_finality_signatures_by_height(tx, height)? {
            None => return Ok(None),
            Some(block_header_with_metadata) => block_header_with_metadata,
        };
        if let Some(block_body) = self.get_body_for_block_header(tx, &block_header)? {
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

    /// Retrieves single block header by height by looking it up in the index and returning it;
    /// returns `None` if they are less than the fault tolerance threshold, or if the block is from
    /// before the most recent emergency upgrade.
    fn get_block_header_and_sufficient_finality_signatures_by_height<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        height: u64,
    ) -> Result<Option<BlockHeaderWithMetadata>, FatalStorageError> {
        let block_hash = match self.block_height_index.get(&height) {
            None => return Ok(None),
            Some(block_hash) => block_hash,
        };
        let block_header = match self.get_single_block_header(tx, block_hash)? {
            None => return Ok(None),
            Some(block_header) => block_header,
        };
        let block_signatures = match self.get_sufficient_finality_signatures(tx, &block_header)? {
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
        let switch_block_hash = match self
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
        let finality_check_result = match switch_block_header.next_era_validator_weights() {
            None => {
                return Err(FatalStorageError::InvalidSwitchBlock(Box::new(
                    switch_block_header,
                )))
            }
            Some(validator_weights) => consensus::check_sufficient_finality_signatures(
                validator_weights,
                self.finality_threshold_fraction,
                &block_signatures,
            ),
        };
        match finality_check_result {
            Err(err @ FinalitySignatureError::InsufficientWeightForFinality { .. }) => {
                info!(
                    ?err,
                    ?block_header,
                    "insufficient finality signatures for block header read from storage"
                );
                Ok(None)
            }
            Err(err) => Err(err.into()),
            Ok(()) => Ok(Some(block_signatures)),
        }
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
        &mut self,
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
            self.serialized_item_pool
                .put(serialized_id.into(), Arc::downgrade(&shared));
        }

        let message = Message::new_get_response_from_serialized(<T as Item>::TAG, shared);
        Ok(effect_builder.send_message(sender, message).ignore())
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
    block_height_index: &mut BTreeMap<u64, BlockHash>,
    switch_block_era_id_index: &mut BTreeMap<EraId, BlockHash>,
    block_header: &BlockHeader,
    verifiable_chunked_hash_activation: EraId,
) -> Result<(), FatalStorageError> {
    let block_hash = block_header.hash(verifiable_chunked_hash_activation);
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
    deploy_hash_index: &mut BTreeMap<DeployHash, BlockHash>,
    block_hash: BlockHash,
    block_body: &BlockBody,
) -> Result<(), FatalStorageError> {
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
        return Err(FatalStorageError::DuplicateDeployIndex {
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

    pub fn env(&self) -> &Environment {
        &self.env
    }

    /// Retrieves single switch block by era ID by looking it up in the index and returning it.
    fn get_switch_block_by_era_id<Tx: Transaction>(
        &self,
        tx: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<Block>, FatalStorageError> {
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
    verifiable_chunked_hash_activation: EraId,
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

    if should_check_integrity {
        let expected_hashing_algorithm_version = HashingAlgorithmVersion::V1;
        for (raw_key, raw_val) in txn.open_ro_cursor(*block_body_v1_db)?.iter() {
            let block_body_hash = Digest::try_from(raw_key)
                .map_err(|err| LmdbExtError::DataCorrupted(Box::new(err)))?;
            let block_body = lmdb_ext::deserialize(raw_val)?;
            if let Some(block_header) = block_body_hash_to_header_map.get(&block_body_hash) {
                let actual_hashing_algorithm_version =
                    block_header.hashing_algorithm_version(verifiable_chunked_hash_activation);
                if expected_hashing_algorithm_version != actual_hashing_algorithm_version {
                    return Err(FatalStorageError::UnexpectedHashingAlgorithmVersion {
                        expected_hashing_algorithm_version,
                        actual_hashing_algorithm_version,
                    });
                }
                // Use smart constructor for block and propagate validation error accordingly
                Block::new_from_header_and_body(
                    block_header.to_owned(),
                    block_body,
                    verifiable_chunked_hash_activation,
                )?;
            } else {
                // Should be unreachable because we just deleted all block bodies that aren't
                // referenced by any header
                return Err(FatalStorageError::NoBlockHeaderForBlockBody {
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
    verifiable_chunked_hash_activation: EraId,
) -> Result<(), FatalStorageError> {
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
            let actual_hashing_algorithm_version =
                block_header.hashing_algorithm_version(verifiable_chunked_hash_activation);
            if expected_hashing_algorithm_version != actual_hashing_algorithm_version {
                return Err(FatalStorageError::UnexpectedHashingAlgorithmVersion {
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
                    Block::new_from_header_and_body(
                        block_header.to_owned(),
                        block_body,
                        verifiable_chunked_hash_activation,
                    )?;
                }
                None => {
                    // get_single_block_body_v2 returning an Ok(None) means we have an
                    // incomplete block body - this doesn't have to indicate an error, it may
                    // be caused by chain sync not downloading the whole body, but only a part
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
) -> Result<(), FatalStorageError> {
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
            signatures
                .verify()
                .map_err(FatalStorageError::SignatureVerification)?;
            if raw_key != signatures.block_hash.as_ref() {
                return Err(FatalStorageError::CorruptedBlockSignatureIndex {
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

    if !deleted_block_hashes.is_empty() {
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
    }

    info!("deploy metadata database initialized");
    Ok(())
}
