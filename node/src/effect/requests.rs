//! Request effects.
//!
//! Requests typically ask other components to perform a service and report back the result. See the
//! top-level module documentation for details.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    mem,
    sync::Arc,
};

use datasize::DataSize;
use serde::Serialize;
use smallvec::SmallVec;

use casper_execution_engine::{
    core::engine_state::{
        self,
        balance::{BalanceRequest, BalanceResult},
        era_validators::GetEraValidatorsError,
        genesis::GenesisSuccess,
        get_bids::{GetBidsRequest, GetBidsResult},
        query::{QueryRequest, QueryResult},
        UpgradeConfig, UpgradeSuccess,
    },
    storage::trie::TrieRaw,
};
use casper_hashing::Digest;
use casper_types::{
    bytesrepr::Bytes, system::auction::EraValidators, EraId, ExecutionResult, Key, ProtocolVersion,
    PublicKey, TimeDiff, Transfer, URef,
};

use crate::{
    components::{
        block_validator::ValidatingBlock,
        consensus::{BlockContext, ClContext, ValidatorChange},
        contract_runtime::{
            BlockAndExecutionEffects, BlockExecutionError, ContractRuntimeError,
            EraValidatorsRequest, ExecutionPreState,
        },
        deploy_acceptor::Error,
        fetcher::FetchResult,
        sync_leaper::ConstructSyncLeapError,
        upgrade_watcher::NextUpgrade,
    },
    contract_runtime::SpeculativeExecutionState,
    effect::{AutoClosingResponder, Responder},
    rpcs::{chain::BlockIdentifier, docs::OpenRpcSchema},
    types::{
        AvailableBlockRange, Block, BlockAndDeploys, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockHeadersBatch, BlockHeadersBatchId, BlockPayload,
        BlockSignatures, BlockWithMetadata, Chainspec, ChainspecRawBytes, Deploy, DeployHash,
        DeployMetadataExt, DeployWithFinalizedApprovals, FetcherItem, FinalitySignature,
        FinalizedApprovals, FinalizedBlock, GossiperItem, NodeId, NodeState, StatusFeed, SyncLeap,
        TrieOrChunk, TrieOrChunkId,
    },
    utils::{DisplayIter, Source},
};

use super::GossipTarget;
// Redirection for reactor macro.
#[allow(unused_imports)]
pub(crate) use super::diagnostics_port::DumpConsensusStateRequest;

const _STORAGE_REQUEST_SIZE: usize = mem::size_of::<StorageRequest>();
// const_assert!(_STORAGE_REQUEST_SIZE < 89);

#[test]
fn size() {
    todo!("fix _STORAGE_REQUEST_SIZE check")
}

/// A metrics request.
#[derive(Debug)]
pub(crate) enum MetricsRequest {
    /// Render current node metrics as prometheus-formatted string.
    RenderNodeMetricsText {
        /// Responder returning the rendered metrics or `None`, if an internal error occurred.
        responder: Responder<Option<String>>,
    },
}

impl Display for MetricsRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MetricsRequest::RenderNodeMetricsText { .. } => write!(formatter, "get metrics text"),
        }
    }
}

const _NETWORK_EVENT_SIZE: usize = mem::size_of::<NetworkRequest<String>>();
//const_assert!(_NETWORK_EVENT_SIZE < 92);

#[test]
fn poop() {
    eprintln!("{}", mem::size_of::<NetworkRequest<String>>());
    panic!("poop is a bad name");
}

// #[derive(Debug, Serialize)]
// pub(crate) enum Excluded {
//     Empty,
//     Explicit(HashSet<NodeId>),
//     ActiveValidators {
//         era_id: EraId,
//         peer_set: HashSet<NodeId>,
//     },
// }

/// A networking request.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) enum NetworkRequest<P> {
    /// Send a message on the network to a specific peer.
    SendMessage {
        /// Message destination.
        dest: Box<NodeId>,
        /// Message payload.
        payload: Box<P>,
        /// If `true`, the responder will be called early after the message has been queued, not
        /// waiting until it has passed to the kernel.
        respond_after_queueing: bool,
        /// Responder to be called when the message has been *buffered for sending*.
        #[serde(skip_serializing)]
        auto_closing_responder: AutoClosingResponder<()>,
    },
    /// Send a message on the network to validator peers in the given era.
    ValidatorBroadcast {
        /// Message payload.
        payload: Box<P>,
        /// Era whose validators are recipients.
        era_id: EraId,
        /// Responder to be called when all messages are queued.
        #[serde(skip_serializing)]
        auto_closing_responder: AutoClosingResponder<()>,
    },
    /// Gossip a message to a random subset of peers.
    Gossip {
        /// Payload to gossip.
        payload: Box<P>,
        /// Type of peers that should receive the gossip message.
        gossip_target: GossipTarget,
        /// Number of peers to gossip to. This is an upper bound, otherwise best-effort.
        count: usize,
        /// Node IDs of nodes to exclude from gossiping to.
        #[serde(skip_serializing)]
        exclude: HashSet<NodeId>,
        // ///
        // gossip_to_validators: bool,
        /// Responder to be called when all messages are queued.
        #[serde(skip_serializing)]
        auto_closing_responder: AutoClosingResponder<HashSet<NodeId>>,
    },
}

impl<P> NetworkRequest<P> {
    /// Transform a network request by mapping the contained payload.
    ///
    /// This is a replacement for a `From` conversion that is not possible without specialization.
    pub(crate) fn map_payload<F, P2>(self, wrap_payload: F) -> NetworkRequest<P2>
    where
        F: FnOnce(P) -> P2,
    {
        match self {
            NetworkRequest::SendMessage {
                dest,
                payload,
                respond_after_queueing,
                auto_closing_responder,
            } => NetworkRequest::SendMessage {
                dest,
                payload: Box::new(wrap_payload(*payload)),
                respond_after_queueing,
                auto_closing_responder,
            },
            NetworkRequest::ValidatorBroadcast {
                payload,
                era_id,
                auto_closing_responder,
            } => NetworkRequest::ValidatorBroadcast {
                payload: Box::new(wrap_payload(*payload)),
                era_id,
                auto_closing_responder,
            },
            NetworkRequest::Gossip {
                payload,
                gossip_target,
                count,
                exclude,
                auto_closing_responder,
            } => NetworkRequest::Gossip {
                payload: Box::new(wrap_payload(*payload)),
                gossip_target,
                count,
                exclude,
                auto_closing_responder,
            },
        }
    }
}

impl<P> Display for NetworkRequest<P>
where
    P: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkRequest::SendMessage { dest, payload, .. } => {
                write!(formatter, "send to {}: {}", dest, payload)
            }
            NetworkRequest::ValidatorBroadcast { payload, .. } => {
                write!(formatter, "broadcast: {}", payload)
            }
            NetworkRequest::Gossip { payload, .. } => write!(formatter, "gossip: {}", payload),
        }
    }
}

/// A networking info request.
#[derive(Debug)]
pub(crate) enum NetworkInfoRequest {
    /// Get incoming and outgoing peers.
    Peers {
        /// Responder to be called with all connected peers.
        /// Responds with a map from [NodeId]s to a socket address, represented as a string.
        responder: Responder<BTreeMap<NodeId, String>>,
    },
    /// Get the peers in random order.
    FullyConnectedPeers {
        /// Responder to be called with all connected in random order peers.
        responder: Responder<Vec<NodeId>>,
    },
    /// Get only non-syncing peers in random order.
    FullyConnectedNonSyncingPeers {
        /// Responder to be called with all connected non-syncing peers in random order.
        responder: Responder<Vec<NodeId>>,
    },
}

impl Display for NetworkInfoRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkInfoRequest::Peers { responder: _ } => {
                write!(formatter, "get peers-to-socket-address map")
            }
            NetworkInfoRequest::FullyConnectedPeers { responder: _ } => {
                write!(formatter, "get fully connected peers")
            }
            NetworkInfoRequest::FullyConnectedNonSyncingPeers { responder: _ } => {
                write!(formatter, "get fully connected non-syncing peers")
            }
        }
    }
}

/// A gossip request.
///
/// This request usually initiates gossiping process of the specified item. Note that the gossiper
/// will fetch the item itself, so only the ID is needed.
///
/// The responder will be called as soon as the gossiper has initiated the process.
// Note: This request should eventually entirely replace `ItemReceived`.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) struct BeginGossipRequest<T>
where
    T: GossiperItem,
{
    /// The ID of the item received.
    pub(crate) item_id: T::Id,
    /// The origin of this request.
    pub(crate) source: Source,
    /// Responder to notify that gossiping is complete.
    pub(crate) responder: Responder<()>,
}

impl<T> Display for BeginGossipRequest<T>
where
    T: GossiperItem,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "begin gossip of {} from {}", self.item_id, self.source)
    }
}

#[derive(Debug, Serialize)]
/// A storage request.
pub(crate) enum StorageRequest {
    /// Store given block.
    PutBlock {
        /// Block to be stored.
        block: Box<Block>,
        /// Responder to call with the result.  Returns true if the block was stored on this
        /// attempt or false if it was previously stored.
        responder: Responder<bool>,
    },
    /// Store given block and its deploys.
    PutBlockAndDeploys {
        /// Block to be stored.
        block: Box<BlockAndDeploys>,
        /// Responder to call on success.  Failure is a fatal error.
        responder: Responder<()>,
    },
    /// Retrieve block with given hash.
    GetBlock {
        /// Hash of block to be retrieved.
        block_hash: BlockHash,
        /// Responder to call with the result.  Returns `None` is the block doesn't exist in local
        /// storage.
        responder: Responder<Option<Block>>,
    },
    /// Retrieve block and deploys with given hash.
    GetBlockAndDeploys {
        /// Hash of block to be retrieved.
        block_hash: BlockHash,
        /// Responder to call with the result.  Returns `None` is the block doesn't exist in local
        /// storage.
        responder: Responder<Option<BlockAndDeploys>>,
    },
    /// Retrieve highest block.
    GetHighestBlock {
        /// Responder.
        responder: Responder<Option<Block>>,
    },
    /// Retrieve highest block header.
    GetHighestBlockHeader {
        /// Responder.
        responder: Responder<Option<BlockHeader>>,
    },
    /// Retrieve switch block header with given era ID.
    GetSwitchBlockHeaderAtEraId {
        /// Era ID of the switch block.
        era_id: EraId,
        /// Responder.
        responder: Responder<Option<BlockHeader>>,
    },
    /// Retrieve the header of the block containing the deploy.
    GetBlockHeaderForDeploy {
        /// Hash of the deploy.
        deploy_hash: DeployHash,
        /// Responder.
        responder: Responder<Option<BlockHeader>>,
    },
    /// Retrieve block header with given hash.
    GetBlockHeader {
        /// Hash of block to get header of.
        block_hash: BlockHash,
        /// Flag indicating whether storage should check the block availability before trying to
        /// retrieve it.
        only_from_available_block_range: bool,
        /// Responder to call with the result.  Returns `None` is the block header doesn't exist in
        /// local storage.
        responder: Responder<Option<BlockHeader>>,
    },
    GetBlockHeaderByHeight {
        /// Height of block to get header of.
        block_height: u64,
        /// Flag indicating whether storage should check the block availability before trying to
        /// retrieve it.
        only_from_available_block_range: bool,
        /// Responder to call with the result.  Returns `None` is the block header doesn't exist in
        /// local storage.
        responder: Responder<Option<BlockHeader>>,
    },
    /// Checks if a block header at the given height exists in storage.
    CheckBlockHeaderExistence {
        /// Height of the block to check.
        block_height: u64,
        /// Responder to call with the result.
        responder: Responder<bool>,
    },
    /// Retrieve all transfers in a block with given hash.
    GetBlockTransfers {
        /// Hash of block to get transfers of.
        block_hash: BlockHash,
        /// Responder to call with the result.  Returns `None` is the transfers do not exist in
        /// local storage under the block_hash provided.
        responder: Responder<Option<Vec<Transfer>>>,
    },
    /// Store given deploy.
    PutDeploy {
        /// Deploy to store.
        deploy: Box<Deploy>,
        /// Responder to call with the result.  Returns true if the deploy was stored on this
        /// attempt or false if it was previously stored.
        responder: Responder<bool>,
    },
    /// Retrieve deploys with given hashes.
    GetDeploys {
        /// Hashes of deploys to be retrieved.
        deploy_hashes: Vec<DeployHash>,
        /// Responder to call with the results.
        responder: Responder<SmallVec<[Option<DeployWithFinalizedApprovals>; 1]>>,
    },
    /// Retrieve finalized blocks that whose deploy TTL hasn't expired yet.
    GetFinalizedBlocks {
        /// Maximum TTL of block we're interested in.
        /// I.e. we don't want deploys from blocks that are older than this.
        ttl: TimeDiff,
        /// Responder to call with the results.
        responder: Responder<Vec<Block>>,
    },
    /// Store execution results for a set of deploys of a single block.
    ///
    /// Will return a fatal error if there are already execution results known for a specific
    /// deploy/block combination and a different result is inserted.
    ///
    /// Inserting the same block/deploy combination multiple times with the same execution results
    /// is not an error and will silently be ignored.
    PutExecutionResults {
        /// Hash of block.
        block_hash: Box<BlockHash>,
        /// Mapping of deploys to execution results of the block.
        execution_results: HashMap<DeployHash, ExecutionResult>,
        /// Responder to call when done storing.
        responder: Responder<()>,
    },
    /// Retrieve deploy and its metadata.
    GetDeployAndMetadata {
        /// Hash of deploy to be retrieved.
        deploy_hash: DeployHash,
        /// Responder to call with the results.
        responder: Responder<Option<(DeployWithFinalizedApprovals, DeployMetadataExt)>>,
    },
    /// Retrieve block and its metadata by its hash.
    GetBlockAndMetadataByHash {
        /// The hash of the block.
        block_hash: BlockHash,
        /// Flag indicating whether storage should check the block availability before trying to
        /// retrieve it.
        only_from_available_block_range: bool,
        /// The responder to call with the results.
        responder: Responder<Option<BlockWithMetadata>>,
    },
    /// Retrieve block header and its metadata by its hash.
    GetBlockHeaderAndMetadataByHash {
        /// The hash of the block.
        block_hash: BlockHash,
        /// Flag indicating whether storage should check the block availability before trying to
        /// retrieve it.
        only_from_available_block_range: bool,
        /// The responder to call with the results.
        responder: Responder<Option<BlockHeaderWithMetadata>>,
    },
    /// Retrieve block and its metadata at a given height.
    GetBlockAndMetadataByHeight {
        /// The height of the block.
        block_height: BlockHeight,
        /// Flag indicating whether storage should check the block availability before trying to
        /// retrieve it.
        only_from_available_block_range: bool,
        /// The responder to call with the results.
        responder: Responder<Option<BlockWithMetadata>>,
    },
    /// Retrieve block header and its metadata at a given height.
    GetBlockHeaderAndMetadataByHeight {
        /// The height of the block.
        block_height: BlockHeight,
        /// Flag indicating whether storage should check the block availability before trying to
        /// retrieve it.
        only_from_available_block_range: bool,
        /// The responder to call with the results.
        responder: Responder<Option<BlockHeaderWithMetadata>>,
    },
    /// Get the highest block and its metadata.
    GetHighestBlockWithMetadata {
        /// The responder to call the results with.
        responder: Responder<Option<BlockWithMetadata>>,
    },
    /// Get finality signatures for a Block hash.
    GetBlockSignatures {
        /// The hash for the request.
        block_hash: BlockHash,
        /// Responder to call with the result.
        responder: Responder<Option<BlockSignatures>>,
    },
    /// Get a single finality signature for a block hash.
    GetBlockSignature {
        /// The hash for the request.
        block_hash: BlockHash,
        /// The public key of the signer.
        public_key: PublicKey,
        /// Responder to call with the result.
        responder: Responder<Option<FinalitySignature>>,
    },
    /// Store finality signatures.
    PutBlockSignatures {
        /// Signatures that are to be stored.
        signatures: BlockSignatures,
        /// Responder to call with the result, if true then the signatures were successfully
        /// stored.
        responder: Responder<bool>,
    },
    /// Store a block header.
    PutBlockHeader {
        /// Block header that is to be stored.
        block_header: Box<BlockHeader>,
        /// Responder to call with the result, if true then the block header was successfully
        /// stored.
        responder: Responder<bool>,
    },
    /// Store a batch of block headers.
    PutHeadersBatch {
        /// Batch of block headers to be stored.
        block_headers: Vec<BlockHeader>,
        /// Responder to call with the result, if true then the block headers were successfully
        /// stored.
        responder: Responder<bool>,
    },
    /// Read batch of block headers by their heights.
    GetHeadersBatch {
        block_headers_id: BlockHeadersBatchId,
        responder: Responder<Option<BlockHeadersBatch>>,
    },
    /// Retrieve the height range of fully available blocks (not just block headers). Returns
    /// `[u64::MAX, u64::MAX]` when there are no sequences.
    GetAvailableBlockRange {
        /// Responder to call with the result.
        responder: Responder<AvailableBlockRange>,
    },
    /// Store a set of finalized approvals for a specific deploy.
    StoreFinalizedApprovals {
        /// The deploy hash to store the finalized approvals for.
        deploy_hash: DeployHash,
        /// The set of finalized approvals.
        finalized_approvals: FinalizedApprovals,
        /// Responder, responded to once the approvals are written.
        responder: Responder<()>,
    },
}

impl Display for StorageRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, .. } => write!(formatter, "put {}", block),
            StorageRequest::PutBlockAndDeploys {
                block: block_deploys,
                ..
            } => {
                write!(
                    formatter,
                    "put block and deploys {}",
                    block_deploys.block.hash()
                )
            }
            StorageRequest::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
            StorageRequest::GetBlockAndDeploys { block_hash, .. } => {
                write!(formatter, "get block and deploys {}", block_hash)
            }
            StorageRequest::GetHighestBlock { .. } => write!(formatter, "get highest block"),
            StorageRequest::GetHighestBlockHeader { .. } => {
                write!(formatter, "get highest block header")
            }
            StorageRequest::GetSwitchBlockHeaderAtEraId { era_id, .. } => {
                write!(formatter, "get switch block header at era id {}", era_id)
            }
            StorageRequest::GetBlockHeaderForDeploy { deploy_hash, .. } => {
                write!(formatter, "get block header for deploy {}", deploy_hash)
            }
            StorageRequest::GetBlockHeader { block_hash, .. } => {
                write!(formatter, "get {}", block_hash)
            }
            StorageRequest::GetBlockHeaderByHeight { block_height, .. } => {
                write!(formatter, "get header for height {}", block_height)
            }
            StorageRequest::CheckBlockHeaderExistence { block_height, .. } => {
                write!(formatter, "check existence {}", block_height)
            }
            StorageRequest::GetBlockTransfers { block_hash, .. } => {
                write!(formatter, "get transfers for {}", block_hash)
            }
            StorageRequest::PutDeploy { deploy, .. } => write!(formatter, "put {}", deploy),
            StorageRequest::GetDeploys { deploy_hashes, .. } => {
                write!(formatter, "get {}", DisplayIter::new(deploy_hashes.iter()))
            }
            StorageRequest::PutExecutionResults { block_hash, .. } => {
                write!(formatter, "put execution results for {}", block_hash)
            }
            StorageRequest::GetDeployAndMetadata { deploy_hash, .. } => {
                write!(formatter, "get deploy and metadata for {}", deploy_hash)
            }
            StorageRequest::GetBlockAndMetadataByHash { block_hash, .. } => {
                write!(
                    formatter,
                    "get block and metadata for block with hash: {}",
                    block_hash
                )
            }
            StorageRequest::GetBlockHeaderAndMetadataByHash { block_hash, .. } => {
                write!(
                    formatter,
                    "get block header and metadata for block with hash: {}",
                    block_hash
                )
            }
            StorageRequest::GetBlockAndMetadataByHeight { block_height, .. } => {
                write!(
                    formatter,
                    "get block and metadata for block at height: {}",
                    block_height
                )
            }
            StorageRequest::GetBlockHeaderAndMetadataByHeight { block_height, .. } => {
                write!(
                    formatter,
                    "get block header and metadata for block at height: {}",
                    block_height
                )
            }
            StorageRequest::GetHighestBlockWithMetadata { .. } => {
                write!(formatter, "get highest block with metadata")
            }
            StorageRequest::GetBlockSignatures { block_hash, .. } => {
                write!(
                    formatter,
                    "get finality signatures for block hash {}",
                    block_hash
                )
            }
            StorageRequest::GetBlockSignature {
                block_hash,
                public_key,
                ..
            } => {
                write!(
                    formatter,
                    "get finality signature for block hash {} from {}",
                    block_hash, public_key
                )
            }
            StorageRequest::PutBlockSignatures { .. } => {
                write!(formatter, "put finality signatures")
            }
            StorageRequest::GetFinalizedBlocks { ttl, .. } => {
                write!(formatter, "get finalized blocks, ttl: {:?}", ttl)
            }
            StorageRequest::PutBlockHeader { block_header, .. } => {
                write!(formatter, "put block header: {}", block_header)
            }
            StorageRequest::GetAvailableBlockRange { .. } => {
                write!(formatter, "get available block range",)
            }
            StorageRequest::StoreFinalizedApprovals { deploy_hash, .. } => {
                write!(formatter, "finalized approvals for deploy {}", deploy_hash)
            }
            StorageRequest::PutHeadersBatch { .. } => {
                write!(formatter, "put block headers batch to storage")
            }
            StorageRequest::GetHeadersBatch {
                block_headers_id, ..
            } => {
                write!(formatter, "get block headers batch: {}", block_headers_id)
            }
        }
    }
}

/// A request to mark a block at a specific height completed.
///
/// A block is considered complete if
///
/// * the block header and the actual block are persisted in storage,
/// * all of its deploys are persisted in storage, and
/// * the global state root the block refers to has no missing dependencies locally.

// Note - this is a request rather than an announcement as the chain synchronizer needs to ensure
// the request has been completed before it can exit, i.e. it awaits the response. Otherwise, the
// joiner reactor might exit before handling the announcement and it would go un-actioned.
#[derive(Debug, Serialize)]
pub(crate) struct MarkBlockCompletedRequest {
    /// Height of the block that was completed.
    pub block_height: u64,
    /// Responder indicating that the change has been recorded.
    pub responder: Responder<()>,
}

impl Display for MarkBlockCompletedRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "block completed: height {}", self.block_height)
    }
}

/// State store request.
#[derive(DataSize, Debug, Serialize)]
#[repr(u8)]
pub(crate) enum StateStoreRequest {
    /// Stores a piece of state to storage.
    Save {
        /// Key to store under.
        key: Cow<'static, [u8]>,
        /// Value to store, already serialized.
        #[serde(skip_serializing)]
        data: Vec<u8>,
        /// Notification when storing is complete.
        responder: Responder<()>,
    },
    /// Loads a piece of state from storage.
    Load {
        /// Key to load from.
        key: Cow<'static, [u8]>,
        /// Responder for value, if found, returning the previously passed in serialization form.
        responder: Responder<Option<Vec<u8>>>,
    },
}

impl Display for StateStoreRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StateStoreRequest::Save { key, data, .. } => {
                write!(
                    f,
                    "save data under {} ({} bytes)",
                    base16::encode_lower(key),
                    data.len()
                )
            }
            StateStoreRequest::Load { key, .. } => {
                write!(f, "load data from key {}", base16::encode_lower(key))
            }
        }
    }
}

/// Details of a request for a list of deploys to propose in a new block.
#[derive(DataSize, Debug)]
pub(crate) struct BlockPayloadRequest {
    /// The context in which the new block will be proposed.
    pub(crate) context: BlockContext<ClContext>,
    /// The height of the next block to be finalized at the point the request was made.
    /// This is _only_ a way of expressing how many blocks have been finalized at the moment the
    /// request was made. Block Proposer uses this in order to determine if there might be any
    /// deploys that are neither in `past_deploys`, nor among the finalized deploys it knows of.
    pub(crate) next_finalized: u64,
    /// A list of validators reported as malicious in this block.
    pub(crate) accusations: Vec<PublicKey>,
    /// Random bit with which to construct the `BlockPayload` requested.
    pub(crate) random_bit: bool,
    /// Responder to call with the result.
    pub(crate) responder: Responder<Arc<BlockPayload>>,
}

/// A `BlockProposer` request.
#[derive(DataSize, Debug)]
#[must_use]
pub(crate) enum BlockProposerRequest {
    /// Request a list of deploys to propose in a new block.
    RequestBlockPayload(BlockPayloadRequest),
}

impl Display for BlockProposerRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockProposerRequest::RequestBlockPayload(BlockPayloadRequest {
                context,
                next_finalized,
                responder: _,
                accusations: _,
                random_bit: _,
            }) => write!(
                formatter,
                "list for inclusion: instant {} height {} next_finalized {}",
                context.timestamp(),
                context.height(),
                next_finalized
            ),
        }
    }
}

/// Abstract RPC request.
///
/// An RPC request is an abstract request that does not concern itself with serialization or
/// transport.
#[derive(Debug)]
#[must_use]
pub(crate) enum RpcRequest {
    /// Submit a deploy to be announced.
    SubmitDeploy {
        /// The deploy to be announced.
        deploy: Box<Deploy>,
        /// Responder to call.
        responder: Responder<Result<(), Error>>,
    },
    /// If `maybe_identifier` is `Some`, return the specified block if it exists, else `None`.  If
    /// `maybe_identifier` is `None`, return the latest block.
    GetBlock {
        /// The identifier (can either be a hash or the height) of the block to be retrieved.
        maybe_id: Option<BlockIdentifier>,
        /// Flag indicating whether storage should check the block availability before trying to
        /// retrieve it.
        only_from_available_block_range: bool,
        /// Responder to call with the result.
        responder: Responder<Option<BlockWithMetadata>>,
    },
    /// Return transfers for block by hash (if any).
    GetBlockTransfers {
        /// The hash of the block to retrieve transfers for.
        block_hash: BlockHash,
        /// Responder to call with the result.
        responder: Responder<Option<Vec<Transfer>>>,
    },
    /// Query the global state at the given root hash.
    QueryGlobalState {
        /// The state root hash.
        state_root_hash: Digest,
        /// Hex-encoded `casper_types::Key`.
        base_key: Key,
        /// The path components starting from the key as base.
        path: Vec<String>,
        /// Responder to call with the result.
        responder: Responder<Result<QueryResult, engine_state::Error>>,
    },
    /// Query the global state at the given root hash.
    QueryEraValidators {
        /// The global state hash.
        state_root_hash: Digest,
        /// The protocol version.
        protocol_version: ProtocolVersion,
        /// Responder to call with the result.
        responder: Responder<Result<EraValidators, GetEraValidatorsError>>,
    },
    /// Get the bids at the given root hash.
    GetBids {
        /// The global state hash.
        state_root_hash: Digest,
        /// Responder to call with the result.
        responder: Responder<Result<GetBidsResult, engine_state::Error>>,
    },

    /// Query the global state at the given root hash.
    GetBalance {
        /// The state root hash.
        state_root_hash: Digest,
        /// The purse URef.
        purse_uref: URef,
        /// Responder to call with the result.
        responder: Responder<Result<BalanceResult, engine_state::Error>>,
    },
    /// Return the specified deploy and metadata if it exists, else `None`.
    GetDeploy {
        /// The hash of the deploy to be retrieved.
        hash: DeployHash,
        /// Whether to return finalized approvals.
        finalized_approvals: bool,
        /// Responder to call with the result.
        responder: Responder<Option<(Deploy, DeployMetadataExt)>>,
    },
    /// Return the connected peers.
    GetPeers {
        /// Responder to call with the result.
        responder: Responder<BTreeMap<NodeId, String>>,
    },
    /// Return string formatted status or `None` if an error occurred.
    GetStatus {
        /// Responder to call with the result.
        responder: Responder<StatusFeed>,
    },
    /// Return the height range of fully available blocks.
    GetAvailableBlockRange {
        /// Responder to call with the result.
        responder: Responder<AvailableBlockRange>,
    },
    /// Executs a deploy against a specified block, returning the effects.
    /// Does not commit the effects. This is a "read-only" action.
    SpeculativeDeployExecute {
        /// Block header representing the state on top of which we will run the deploy.
        block_header: BlockHeader,
        /// Deploy to execute.
        deploy: Box<Deploy>,
        /// Responder.
        responder: Responder<Result<Option<ExecutionResult>, engine_state::Error>>,
    },
}

impl Display for RpcRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RpcRequest::SubmitDeploy { deploy, .. } => write!(formatter, "submit {}", *deploy),
            RpcRequest::GetBlock {
                maybe_id: Some(BlockIdentifier::Hash(hash)),
                ..
            } => write!(formatter, "get {}", hash),
            RpcRequest::GetBlock {
                maybe_id: Some(BlockIdentifier::Height(height)),
                ..
            } => write!(formatter, "get {}", height),
            RpcRequest::GetBlock { maybe_id: None, .. } => write!(formatter, "get latest block"),
            RpcRequest::GetBlockTransfers { block_hash, .. } => {
                write!(formatter, "get transfers {}", block_hash)
            }

            RpcRequest::QueryGlobalState {
                state_root_hash,
                base_key,
                path,
                ..
            } => write!(
                formatter,
                "query {}, base_key: {}, path: {:?}",
                state_root_hash, base_key, path
            ),
            RpcRequest::QueryEraValidators {
                state_root_hash, ..
            } => write!(formatter, "auction {}", state_root_hash),
            RpcRequest::GetBids {
                state_root_hash, ..
            } => {
                write!(formatter, "bids {}", state_root_hash)
            }
            RpcRequest::GetBalance {
                state_root_hash,
                purse_uref,
                ..
            } => write!(
                formatter,
                "balance {}, purse_uref: {}",
                state_root_hash, purse_uref
            ),
            RpcRequest::GetDeploy {
                hash,
                finalized_approvals,
                ..
            } => write!(
                formatter,
                "get {} (finalized approvals: {})",
                hash, finalized_approvals
            ),
            RpcRequest::GetPeers { .. } => write!(formatter, "get peers"),
            RpcRequest::GetStatus { .. } => write!(formatter, "get status"),
            RpcRequest::GetAvailableBlockRange { .. } => {
                write!(formatter, "get available block range")
            }
            RpcRequest::SpeculativeDeployExecute { .. } => write!(formatter, "execute deploy"),
        }
    }
}

/// Abstract REST request.
///
/// An REST request is an abstract request that does not concern itself with serialization or
/// transport.
#[derive(Debug)]
#[must_use]
pub(crate) enum RestRequest {
    /// Return string formatted status or `None` if an error occurred.
    Status {
        /// Responder to call with the result.
        responder: Responder<StatusFeed>,
    },
    /// Return string formatted, prometheus compatible metrics or `None` if an error occurred.
    Metrics {
        /// Responder to call with the result.
        responder: Responder<Option<String>>,
    },
    /// Returns schema of client-facing JSON-RPCs in OpenRPC format.
    RpcSchema {
        /// Responder to call with the result
        responder: Responder<OpenRpcSchema>,
    },
}

impl Display for RestRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RestRequest::Status { .. } => write!(formatter, "get status"),
            RestRequest::Metrics { .. } => write!(formatter, "get metrics"),
            RestRequest::RpcSchema { .. } => write!(formatter, "get openrpc"),
        }
    }
}

/// A contract runtime request.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) enum ContractRuntimeRequest {
    /// A request to enqueue a `FinalizedBlock` for execution.
    EnqueueBlockForExecution {
        /// A `FinalizedBlock` to enqueue.
        finalized_block: FinalizedBlock,
        /// The deploys for that `FinalizedBlock`
        deploys: Vec<Deploy>,
        /// The transfers for that `FinalizedBlock`
        transfers: Vec<Deploy>,
    },
    /// Commit genesis chainspec.
    CommitGenesis {
        /// The chainspec.
        chainspec: Arc<Chainspec>,
        /// The chainspec files' raw bytes.
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        /// Responder to call with the result.
        responder: Responder<Result<GenesisSuccess, engine_state::Error>>,
    },
    /// A request to run upgrade.
    Upgrade {
        /// Upgrade config.
        #[serde(skip_serializing)]
        upgrade_config: Box<UpgradeConfig>,
        /// Responder to call with the upgrade result.
        responder: Responder<Result<UpgradeSuccess, engine_state::Error>>,
    },
    /// A query request.
    Query {
        /// Query request.
        #[serde(skip_serializing)]
        query_request: QueryRequest,
        /// Responder to call with the query result.
        responder: Responder<Result<QueryResult, engine_state::Error>>,
    },
    /// A balance request.
    GetBalance {
        /// Balance request.
        #[serde(skip_serializing)]
        balance_request: BalanceRequest,
        /// Responder to call with the balance result.
        responder: Responder<Result<BalanceResult, engine_state::Error>>,
    },
    /// Returns validator weights.
    GetEraValidators {
        /// Get validators weights request.
        #[serde(skip_serializing)]
        request: EraValidatorsRequest,
        /// Responder to call with the result.
        responder: Responder<Result<EraValidators, GetEraValidatorsError>>,
    },
    /// Return bids at a given state root hash
    GetBids {
        /// Get bids request.
        #[serde(skip_serializing)]
        get_bids_request: GetBidsRequest,
        /// Responder to call with the result.
        responder: Responder<Result<GetBidsResult, engine_state::Error>>,
    },
    /// Check if validator is bonded in the future era (identified by `era_id`).
    IsBonded {
        /// State root hash of the LFB.
        state_root_hash: Digest,
        /// Validator public key.
        public_key: PublicKey,
        /// Era ID in which validator should be bonded in.
        era_id: EraId,
        /// Protocol version at the `state_root_hash`.
        protocol_version: ProtocolVersion,
        /// Responder,
        responder: Responder<Result<bool, GetEraValidatorsError>>,
    },
    /// Get a trie or chunk by its ID.
    GetTrie {
        /// The ID of the trie (or chunk of a trie) to be read.
        trie_or_chunk_id: TrieOrChunkId,
        /// Responder to call with the result.
        responder: Responder<Result<Option<TrieOrChunk>, ContractRuntimeError>>,
    },
    /// Get a trie by its ID.
    GetTrieFull {
        /// The ID of the trie to be read.
        trie_key: Digest,
        /// Responder to call with the result.
        responder: Responder<Result<Option<Bytes>, engine_state::Error>>,
    },
    /// Insert a trie into global storage
    PutTrie {
        /// The hash of the value to get from the `TrieStore`
        trie_bytes: TrieRaw,
        /// Responder to call with the result. Contains the missing descendants of the inserted
        /// trie.
        responder: Responder<Result<Vec<Digest>, engine_state::Error>>,
    },
    /// Find the missing descendants for a trie key
    FindMissingDescendantTrieKeys {
        /// The trie key to find the missing descendants for.
        trie_key: Digest,
        /// The responder to call with the result.
        responder: Responder<Result<Vec<Digest>, engine_state::Error>>,
    },
    /// Execute a provided protoblock
    ExecuteBlock {
        /// The protocol version of the block to execute.
        protocol_version: ProtocolVersion,
        /// The state of the storage and blockchain to use to make the new block.
        execution_pre_state: ExecutionPreState,
        /// The finalized block to execute; must have the same height as the child height specified
        /// by the `execution_pre_state`.
        finalized_block: FinalizedBlock,
        /// The deploys for the block to execute; must correspond to the deploy hashes of the
        /// `finalized_block` in that order.
        deploys: Vec<Deploy>,
        /// The transfers for the block to execute; must correspond to the transfer hashes of the
        /// `finalized_block` in that order.
        transfers: Vec<Deploy>,
        /// Responder to call with the result.
        responder: Responder<Result<BlockAndExecutionEffects, BlockExecutionError>>,
    },
    /// Execute deploys without commiting results
    SpeculativeDeployExecution {
        /// Hash of a block on top of which to execute the deploy.
        execution_prestate: SpeculativeExecutionState,
        /// Deploy to execute.
        deploy: Box<Deploy>,
        /// Results
        responder: Responder<Result<Option<ExecutionResult>, engine_state::Error>>,
    },
}

impl Display for ContractRuntimeRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeRequest::EnqueueBlockForExecution {
                finalized_block,
                deploys: _,
                transfers: _,
            } => {
                write!(formatter, "finalized_block: {}", finalized_block)
            }

            ContractRuntimeRequest::CommitGenesis { chainspec, .. } => {
                write!(
                    formatter,
                    "commit genesis {}",
                    chainspec.protocol_config.version
                )
            }

            ContractRuntimeRequest::Upgrade { upgrade_config, .. } => {
                write!(formatter, "upgrade request: {:?}", upgrade_config)
            }

            ContractRuntimeRequest::Query { query_request, .. } => {
                write!(formatter, "query request: {:?}", query_request)
            }

            ContractRuntimeRequest::GetBalance {
                balance_request, ..
            } => write!(formatter, "balance request: {:?}", balance_request),

            ContractRuntimeRequest::GetEraValidators { request, .. } => {
                write!(formatter, "get era validators: {:?}", request)
            }

            ContractRuntimeRequest::GetBids {
                get_bids_request, ..
            } => {
                write!(formatter, "get bids request: {:?}", get_bids_request)
            }

            ContractRuntimeRequest::IsBonded {
                public_key, era_id, ..
            } => {
                write!(formatter, "is {} bonded in era {}", public_key, era_id)
            }
            ContractRuntimeRequest::GetTrie {
                trie_or_chunk_id, ..
            } => {
                write!(formatter, "get trie_or_chunk_id: {}", trie_or_chunk_id)
            }
            ContractRuntimeRequest::GetTrieFull { trie_key, .. } => {
                write!(formatter, "get trie_key: {}", trie_key)
            }
            ContractRuntimeRequest::PutTrie { trie_bytes, .. } => {
                write!(formatter, "trie: {:?}", trie_bytes)
            }
            ContractRuntimeRequest::ExecuteBlock {
                finalized_block, ..
            } => {
                write!(formatter, "Execute finalized block: {}", finalized_block)
            }
            ContractRuntimeRequest::FindMissingDescendantTrieKeys { trie_key, .. } => {
                write!(formatter, "Find missing descendant trie keys: {}", trie_key)
            }
            ContractRuntimeRequest::SpeculativeDeployExecution {
                execution_prestate,
                deploy,
                ..
            } => {
                write!(
                    formatter,
                    "Execute {} on {}",
                    deploy.id(),
                    execution_prestate.state_root_hash
                )
            }
        }
    }
}

/// Fetcher related requests.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) struct FetcherRequest<T: FetcherItem> {
    /// The ID of the item to be retrieved.
    pub(crate) id: T::Id,
    /// The peer id of the peer to be asked if the item is not held locally
    pub(crate) peer: NodeId,
    /// Metadata used during validation of the fetched item.
    pub(crate) validation_metadata: T::ValidationMetadata,
    /// Responder to call with the result.
    pub(crate) responder: Responder<FetchResult<T>>,
}

impl<T: FetcherItem> Display for FetcherRequest<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "request item by id {}", self.id)
    }
}

/// A block validator request.
#[derive(Debug)]
#[must_use]
pub(crate) struct BlockValidationRequest {
    /// The block to be validated.
    pub(crate) block: ValidatingBlock,
    /// The sender of the block, which will be asked to provide all missing deploys.
    pub(crate) sender: NodeId,
    /// Responder to call with the result.
    ///
    /// Indicates whether or not validation was successful.
    pub(crate) responder: Responder<bool>,
}

impl Display for BlockValidationRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let BlockValidationRequest { block, sender, .. } = self;
        write!(f, "validate block {} from {}", block, sender)
    }
}

type BlockHeight = u64;

#[derive(DataSize, Debug)]
#[must_use]
/// Consensus component requests.
pub(crate) enum ConsensusRequest {
    /// Request for our public key, and if we're a validator, the next round length.
    Status(Responder<Option<(PublicKey, Option<TimeDiff>)>>),
    /// Request for a list of validator status changes, by public key.
    ValidatorChanges(Responder<BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>>),
}

/// ChainspecLoader component requests.
#[derive(Debug, Serialize)]
pub(crate) enum ChainspecRawBytesRequest {
    /// Request for the chainspec file bytes with the genesis_accounts and global_state bytes, if
    /// they are present.
    GetChainspecRawBytes(Responder<Arc<ChainspecRawBytes>>),
}

impl Display for ChainspecRawBytesRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ChainspecRawBytesRequest::GetChainspecRawBytes(_) => {
                write!(f, "get chainspec raw bytes")
            }
        }
    }
}

/// UpgradeWatcher component request to get the next scheduled upgrade, if any.
#[derive(Debug, Serialize)]
pub(crate) struct UpgradeWatcherRequest(pub(crate) Responder<Option<NextUpgrade>>);

impl Display for UpgradeWatcherRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "get next upgrade")
    }
}

/// ChainSynchronizer component request.
#[derive(Debug, Serialize)]
pub(crate) struct NodeStateRequest(pub(crate) Responder<NodeState>);

impl Display for NodeStateRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "node state request")
    }
}

/// A request for a sync leap
#[derive(Debug, Serialize)]
pub(crate) struct SyncLeapRequest {
    pub(crate) trusted_hash: BlockHash,
    pub(crate) responder: Responder<Result<SyncLeap, ConstructSyncLeapError>>,
}

impl Display for SyncLeapRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "sync leap request")
    }
}
