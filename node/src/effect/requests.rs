//! Request effects.
//!
//! Requests typically ask other components to perform a service and report back the result. See the
//! top-level module documentation for details.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    mem,
    sync::Arc,
};

use datasize::DataSize;
use serde::Serialize;
use static_assertions::const_assert;

use casper_execution_engine::{
    core::engine_state::{
        self,
        balance::{BalanceRequest, BalanceResult},
        era_validators::GetEraValidatorsError,
        query::{GetBidsRequest, GetBidsResult, QueryRequest, QueryResult},
    },
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::Trie,
};
use casper_types::{
    system::auction::EraValidators, EraId, ExecutionResult, Key, ProtocolVersion, PublicKey,
    Transfer, URef,
};

use crate::{
    components::{
        block_validator::ValidatingBlock,
        chainspec_loader::CurrentRunInfo,
        consensus::{BlockContext, ClContext},
        contract_runtime::{
            BlockAndExecutionEffects, BlockExecutionError, EraValidatorsRequest, ExecutionPreState,
        },
        deploy_acceptor::Error,
        fetcher::FetchResult,
    },
    crypto::hash::Digest,
    effect::Responder,
    rpcs::{chain::BlockIdentifier, docs::OpenRpcSchema},
    types::{
        Block, BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockPayload, BlockSignatures,
        BlockWithMetadata, ChainspecInfo, Deploy, DeployHash, DeployHeader, DeployMetadata,
        FinalizedBlock, Item, NodeId, StatusFeed, TimeDiff,
    },
    utils::DisplayIter,
};

const _STORAGE_REQUEST_SIZE: usize = mem::size_of::<StorageRequest>();
const_assert!(_STORAGE_REQUEST_SIZE < 89);

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

const _NETWORK_EVENT_SIZE: usize = mem::size_of::<NetworkRequest<NodeId, String>>();
const_assert!(_NETWORK_EVENT_SIZE < 89);

/// A networking request.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) enum NetworkRequest<I, P> {
    /// Send a message on the network to a specific peer.
    SendMessage {
        /// Message destination.
        dest: Box<I>,
        /// Message payload.
        payload: Box<P>,
        /// Responder to be called when the message is queued.
        #[serde(skip_serializing)]
        responder: Responder<()>,
    },
    /// Send a message on the network to all peers.
    /// Note: This request is deprecated and should be phased out, as not every network
    ///       implementation is likely to implement broadcast support.
    Broadcast {
        /// Message payload.
        payload: Box<P>,
        /// Responder to be called when all messages are queued.
        #[serde(skip_serializing)]
        responder: Responder<()>,
    },
    /// Gossip a message to a random subset of peers.
    Gossip {
        /// Payload to gossip.
        payload: Box<P>,
        /// Number of peers to gossip to. This is an upper bound, otherwise best-effort.
        count: usize,
        /// Node IDs of nodes to exclude from gossiping to.
        #[serde(skip_serializing)]
        exclude: HashSet<I>,
        /// Responder to be called when all messages are queued.
        #[serde(skip_serializing)]
        responder: Responder<HashSet<I>>,
    },
}

impl<I, P> NetworkRequest<I, P> {
    /// Transform a network request by mapping the contained payload.
    ///
    /// This is a replacement for a `From` conversion that is not possible without specialization.
    pub(crate) fn map_payload<F, P2>(self, wrap_payload: F) -> NetworkRequest<I, P2>
    where
        F: FnOnce(P) -> P2,
    {
        match self {
            NetworkRequest::SendMessage {
                dest,
                payload,
                responder,
            } => NetworkRequest::SendMessage {
                dest,
                payload: Box::new(wrap_payload(*payload)),
                responder,
            },
            NetworkRequest::Broadcast { payload, responder } => NetworkRequest::Broadcast {
                payload: Box::new(wrap_payload(*payload)),
                responder,
            },
            NetworkRequest::Gossip {
                payload,
                count,
                exclude,
                responder,
            } => NetworkRequest::Gossip {
                payload: Box::new(wrap_payload(*payload)),
                count,
                exclude,
                responder,
            },
        }
    }
}

impl<I, P> Display for NetworkRequest<I, P>
where
    I: Display,
    P: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkRequest::SendMessage { dest, payload, .. } => {
                write!(formatter, "send to {}: {}", dest, payload)
            }
            NetworkRequest::Broadcast { payload, .. } => {
                write!(formatter, "broadcast: {}", payload)
            }
            NetworkRequest::Gossip { payload, .. } => write!(formatter, "gossip: {}", payload),
        }
    }
}

/// A networking info request.
#[derive(Debug)]
#[must_use]
pub(crate) enum NetworkInfoRequest<I> {
    /// Get incoming and outgoing peers.
    GetPeers {
        /// Responder to be called with all connected peers.
        /// Responds with a map from [NodeId]s to a socket address, represented as a string.
        responder: Responder<BTreeMap<I, String>>,
    },

    /// Get the peers in random order.
    GetPeersInRandomOrder {
        /// Responder to be called with all connected peers.
        /// Responds with a vector in a random order.
        responder: Responder<Vec<I>>,
    },
}

impl<I> Display for NetworkInfoRequest<I>
where
    I: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkInfoRequest::GetPeers { responder: _ } => {
                write!(formatter, "get peers-to-socket-address map")
            }
            NetworkInfoRequest::GetPeersInRandomOrder { responder: _ } => {
                write!(formatter, "get peers in random order")
            }
        }
    }
}

#[derive(Debug, Serialize)]
/// A storage request.
#[must_use]
#[repr(u8)]
pub(crate) enum StorageRequest {
    /// Store given block.
    PutBlock {
        /// Block to be stored.
        block: Box<Block>,
        /// Responder to call with the result.  Returns true if the block was stored on this
        /// attempt or false if it was previously stored.
        responder: Responder<bool>,
    },
    /// Retrieve block with given hash.
    GetBlock {
        /// Hash of block to be retrieved.
        block_hash: BlockHash,
        /// Responder to call with the result.  Returns `None` is the block doesn't exist in local
        /// storage.
        responder: Responder<Option<Block>>,
    },
    /// Retrieve highest block.
    GetHighestBlock {
        /// Responder.
        responder: Responder<Option<Block>>,
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
        /// Responder to call with the result.  Returns `None` is the block header doesn't exist in
        /// local storage.
        responder: Responder<Option<BlockHeader>>,
    },
    /// Retrieve block header with sufficient finality signatures by height.
    GetBlockHeaderAndSufficientFinalitySignaturesByHeight {
        /// Height of block to get header of.
        block_height: u64,
        /// Responder to call with the result.  Returns `None` if the block header doesn't exist in
        /// local storage.
        responder: Responder<Option<BlockHeaderWithMetadata>>,
    },
    /// Retrieves a block header with sufficient finality signatures by height.
    GetBlockAndSufficientFinalitySignaturesByHeight {
        /// Height of block to get header of.
        block_height: u64,
        /// Responder to call with the result.  Returns `None` if the block header doesn't exist or
        /// does not have sufficient finality signatures by height.
        responder: Responder<Option<BlockWithMetadata>>,
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
        responder: Responder<Vec<Option<Deploy>>>,
    },
    /// Retrieve deploys that are finalized and whose TTL hasn't expired yet.
    GetFinalizedDeploys {
        /// Maximum TTL of block we're interested in.
        /// I.e. we don't want deploys from blocks that are older than this.
        ttl: TimeDiff,
        /// Responder to call with the results.
        responder: Responder<Vec<(DeployHash, DeployHeader)>>,
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
        responder: Responder<Option<(Deploy, DeployMetadata)>>,
    },
    /// Retrieve block and its metadata by its hash.
    GetBlockAndMetadataByHash {
        /// The hash of the block.
        block_hash: BlockHash,
        /// The responder to call with the results.
        responder: Responder<Option<BlockWithMetadata>>,
    },
    /// Retrieve block and its metadata at a given height.
    GetBlockAndMetadataByHeight {
        /// The height of the block.
        block_height: BlockHeight,
        /// The responder to call with the results.
        responder: Responder<Option<BlockWithMetadata>>,
    },
    /// Get the highest block and its metadata.
    GetHighestBlockWithMetadata {
        /// The responder to call the results with.
        responder: Responder<Option<BlockWithMetadata>>,
    },
    /// Get finality signatures for a Block hash.
    GetBlockSignatures {
        /// The hash for the request
        block_hash: BlockHash,
        /// Responder to call with the result.
        responder: Responder<Option<BlockSignatures>>,
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
}

impl Display for StorageRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, .. } => write!(formatter, "put {}", block),
            StorageRequest::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
            StorageRequest::GetHighestBlock { .. } => write!(formatter, "get highest block"),
            StorageRequest::GetSwitchBlockHeaderAtEraId { era_id, .. } => {
                write!(formatter, "get switch block header at era id {}", era_id)
            }
            StorageRequest::GetBlockHeaderForDeploy { deploy_hash, .. } => {
                write!(formatter, "get block header for deploy {}", deploy_hash)
            }
            StorageRequest::GetBlockHeader { block_hash, .. } => {
                write!(formatter, "get {}", block_hash)
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
            StorageRequest::GetBlockAndMetadataByHeight { block_height, .. } => {
                write!(
                    formatter,
                    "get block and metadata for block at height: {}",
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
            StorageRequest::PutBlockSignatures { .. } => {
                write!(formatter, "put finality signatures")
            }
            StorageRequest::GetFinalizedDeploys { ttl, .. } => {
                write!(formatter, "get finalized deploys, ttl: {:?}", ttl)
            }
            StorageRequest::GetBlockHeaderAndSufficientFinalitySignaturesByHeight {
                block_height,
                ..
            } => {
                write!(
                    formatter,
                    "get block and metadata for block by height: {}",
                    block_height
                )
            }
            StorageRequest::PutBlockHeader { block_header, .. } => {
                write!(formatter, "put block header: {}", block_header)
            }
            StorageRequest::GetBlockAndSufficientFinalitySignaturesByHeight {
                block_height,
                ..
            } => {
                write!(
                    formatter,
                    "get block and sufficient finality signatures by height: {}",
                    block_height
                )
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
pub(crate) enum RpcRequest<I> {
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
        /// Responder to call with the result.
        responder: Responder<Option<(Deploy, DeployMetadata)>>,
    },
    /// Return the connected peers.
    GetPeers {
        /// Responder to call with the result.
        responder: Responder<BTreeMap<I, String>>,
    },
    /// Return string formatted status or `None` if an error occurred.
    GetStatus {
        /// Responder to call with the result.
        responder: Responder<StatusFeed<I>>,
    },
}

impl<I> Display for RpcRequest<I> {
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
            RpcRequest::GetDeploy { hash, .. } => write!(formatter, "get {}", hash),
            RpcRequest::GetPeers { .. } => write!(formatter, "get peers"),
            RpcRequest::GetStatus { .. } => write!(formatter, "get status"),
        }
    }
}

/// Abstract REST request.
///
/// An REST request is an abstract request that does not concern itself with serialization or
/// transport.
#[derive(Debug)]
#[must_use]
pub(crate) enum RestRequest<I> {
    /// Return string formatted status or `None` if an error occurred.
    Status {
        /// Responder to call with the result.
        responder: Responder<StatusFeed<I>>,
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

impl<I> Display for RestRequest<I> {
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
    /// Read a trie by its hash key
    ReadTrie {
        /// The hash of the value to get from the `TrieStore`
        trie_key: Blake2bHash,
        /// Responder to call with the result.
        responder: Responder<Option<Trie<Key, StoredValue>>>,
    },
    /// Insert a trie into global storage
    PutTrie {
        /// The hash of the value to get from the `TrieStore`
        trie: Box<Trie<Key, StoredValue>>,
        /// Responder to call with the result. Contains the missing descendants of the inserted
        /// trie.
        responder: Responder<Result<Vec<Blake2bHash>, engine_state::Error>>,
    },
    /// Find the missing descendants for a trie key
    FindMissingDescendantTrieKeys {
        /// The trie key to find the missing descendants for.
        trie_key: Blake2bHash,
        /// The responder to call with the result.
        responder: Responder<Result<Vec<Blake2bHash>, engine_state::Error>>,
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
            ContractRuntimeRequest::ReadTrie { trie_key, .. } => {
                write!(formatter, "get trie_key: {}", trie_key)
            }
            ContractRuntimeRequest::PutTrie { trie, .. } => {
                write!(formatter, "trie: {:?}", trie)
            }
            ContractRuntimeRequest::ExecuteBlock {
                finalized_block, ..
            } => {
                write!(formatter, "Execute finalized block: {}", finalized_block)
            }
            ContractRuntimeRequest::FindMissingDescendantTrieKeys { trie_key, .. } => {
                write!(formatter, "Find missing descendant trie keys: {}", trie_key)
            }
        }
    }
}

/// Fetcher related requests.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) struct FetcherRequest<I, T>
where
    T: Item,
    I: Debug + Eq,
{
    /// The ID of the item to be retrieved.
    pub(crate) id: T::Id,
    /// The peer id of the peer to be asked if the item is not held locally
    pub(crate) peer: I,
    /// Responder to call with the result.
    pub(crate) responder: Responder<FetchResult<T, I>>,
}

impl<I: Display + Debug + Eq, T: Item> Display for FetcherRequest<I, T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "request item by id {}", self.id)
    }
}

/// A block validator request.
#[derive(Debug)]
#[must_use]
pub(crate) struct BlockValidationRequest<I> {
    /// The block to be validated.
    pub(crate) block: ValidatingBlock,
    /// The sender of the block, which will be asked to provide all missing deploys.
    pub(crate) sender: I,
    /// Responder to call with the result.
    ///
    /// Indicates whether or not validation was successful.
    pub(crate) responder: Responder<bool>,
}

impl<I: Display> Display for BlockValidationRequest<I> {
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
}

/// ChainspecLoader component requests.
#[derive(Debug, Serialize)]
pub(crate) enum ChainspecLoaderRequest {
    /// Chainspec info request.
    GetChainspecInfo(Responder<ChainspecInfo>),
    /// Request for information about the current run.
    GetCurrentRunInfo(Responder<CurrentRunInfo>),
}

impl Display for ChainspecLoaderRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainspecLoaderRequest::GetChainspecInfo(_) => write!(f, "get chainspec info"),
            ChainspecLoaderRequest::GetCurrentRunInfo(_) => write!(f, "get current run info"),
        }
    }
}
