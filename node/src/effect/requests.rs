//! Request effects.
//!
//! Requests typically ask other components to perform a service and report back the result. See the
//! top-level module documentation for details.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
};

use datasize::DataSize;
use hex_fmt::HexFmt;
use semver::Version;
use serde::Serialize;

use casper_execution_engine::{
    core::engine_state::{
        self,
        balance::{BalanceRequest, BalanceResult},
        era_validators::GetEraValidatorsError,
        execute_request::ExecuteRequest,
        execution_result::ExecutionResults,
        genesis::GenesisResult,
        query::{QueryRequest, QueryResult},
        step::{StepRequest, StepResult},
        upgrade::{UpgradeConfig, UpgradeResult},
    },
    shared::{additive_map::AdditiveMap, transform::Transform},
    storage::{global_state::CommitResult, protocol_data::ProtocolData},
};
use casper_types::{
    auction::{EraValidators, ValidatorWeights},
    ExecutionResult, Key, ProtocolVersion, PublicKey, Transfer, URef,
};

use super::{Multiple, Responder};
use crate::{
    components::{
        chainspec_loader::ChainspecInfo,
        consensus::EraId,
        contract_runtime::{EraValidatorsRequest, ValidatorWeightsByEraIdRequest},
        deploy_acceptor::Error,
        fetcher::FetchResult,
    },
    crypto::hash::Digest,
    rpcs::chain::BlockIdentifier,
    types::{
        Block as LinearBlock, Block, BlockHash, BlockHeader, BlockSignatures, Chainspec, Deploy,
        DeployHash, DeployHeader, DeployMetadata, FinalitySignature, FinalizedBlock, Item,
        ProtoBlock, StatusFeed, Timestamp,
    },
    utils::DisplayIter,
};
use casper_execution_engine::{
    core::engine_state::put_trie::InsertedTrieKeyAndMissingDescendants,
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::Trie,
};

/// A metrics request.
#[derive(Debug)]
pub enum MetricsRequest {
    /// Render current node metrics as prometheus-formatted string.
    RenderNodeMetricsText {
        /// Resopnder returning the rendered metrics or `None`, if an internal error occurred.
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

/// A networking request.
#[derive(Debug, Serialize)]
#[must_use]
pub enum NetworkRequest<I, P> {
    /// Send a message on the network to a specific peer.
    SendMessage {
        /// Message destination.
        dest: I,
        /// Message payload.
        payload: P,
        /// Responder to be called when the message is queued.
        #[serde(skip_serializing)]
        responder: Responder<()>,
    },
    /// Send a message on the network to all peers.
    /// Note: This request is deprecated and should be phased out, as not every network
    ///       implementation is likely to implement broadcast support.
    Broadcast {
        /// Message payload.
        payload: P,
        /// Responder to be called when all messages are queued.
        #[serde(skip_serializing)]
        responder: Responder<()>,
    },
    /// Gossip a message to a random subset of peers.
    Gossip {
        /// Payload to gossip.
        payload: P,
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
                payload: wrap_payload(payload),
                responder,
            },
            NetworkRequest::Broadcast { payload, responder } => NetworkRequest::Broadcast {
                payload: wrap_payload(payload),
                responder,
            },
            NetworkRequest::Gossip {
                payload,
                count,
                exclude,
                responder,
            } => NetworkRequest::Gossip {
                payload: wrap_payload(payload),
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
pub enum NetworkInfoRequest<I> {
    /// Get incoming and outgoing peers.
    GetPeers {
        /// Responder to be called with all connected peers.
        // TODO - change the `String` field to a `libp2p::Multiaddr` once small_network is removed.
        responder: Responder<BTreeMap<I, String>>,
    },
}

impl<I> Display for NetworkInfoRequest<I>
where
    I: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkInfoRequest::GetPeers { responder: _ } => write!(formatter, "get peers"),
        }
    }
}

#[derive(Debug, Serialize)]
/// A storage request.
#[must_use]
pub enum StorageRequest {
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
    /// Retrieve block with given height.
    GetBlockAtHeight {
        /// Height of the block.
        height: BlockHeight,
        /// Responder.
        responder: Responder<Option<Block>>,
    },
    /// Retrieve highest block.
    GetHighestBlock {
        /// Responder.
        responder: Responder<Option<Block>>,
    },
    /// Retrieve switch block with given era ID.
    GetSwitchBlockAtEraId {
        /// Era ID of the switch block.
        era_id: EraId,
        /// Responder.
        responder: Responder<Option<Block>>,
    },
    /// Retrieve highest switch block.
    GetHighestSwitchBlock {
        /// Responder.
        responder: Responder<Option<Block>>,
    },
    /// Retrieve block header with given hash.
    GetBlockHeader {
        /// Hash of block to get header of.
        block_hash: BlockHash,
        /// Responder to call with the result.  Returns `None` is the block header doesn't exist in
        /// local storage.
        responder: Responder<Option<BlockHeader>>,
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
        deploy_hashes: Multiple<DeployHash>,
        /// Responder to call with the results.
        responder: Responder<Vec<Option<Deploy>>>,
    },
    /// Retrieve deploy headers with given hashes.
    GetDeployHeaders {
        /// Hashes of deploy headers to be retrieved.
        deploy_hashes: Multiple<DeployHash>,
        /// Responder to call with the results.
        responder: Responder<Vec<Option<DeployHeader>>>,
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
        block_hash: BlockHash,
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
        responder: Responder<Option<(Block, BlockSignatures)>>,
    },
    /// Retrieve block and its metadata at a given height.
    GetBlockAndMetadataByHeight {
        /// The height of the block.
        block_height: BlockHeight,
        /// The responder to call with the results.
        responder: Responder<Option<(Block, BlockSignatures)>>,
    },
    /// Get the highest block and its metadata.
    GetHighestBlockWithMetadata {
        /// The responder to call the results with.
        responder: Responder<Option<(Block, BlockSignatures)>>,
    },
    /// Store given chainspec.
    PutChainspec {
        /// Chainspec.
        chainspec: Arc<Chainspec>,
        /// Responder to call with the result.
        responder: Responder<()>,
    },
    /// Retrieve chainspec with given version.
    GetChainspec {
        /// Version.
        version: Version,
        /// Responder to call with the result.
        responder: Responder<Option<Arc<Chainspec>>>,
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
}

impl Display for StorageRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, .. } => write!(formatter, "put {}", block),
            StorageRequest::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
            StorageRequest::GetBlockAtHeight { height, .. } => {
                write!(formatter, "get block at height {}", height)
            }
            StorageRequest::GetHighestBlock { .. } => write!(formatter, "get highest block"),
            StorageRequest::GetSwitchBlockAtEraId { era_id, .. } => {
                write!(formatter, "get switch block at era id {}", era_id)
            }
            StorageRequest::GetHighestSwitchBlock { .. } => {
                write!(formatter, "get highest switch block")
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
            StorageRequest::GetDeployHeaders { deploy_hashes, .. } => write!(
                formatter,
                "get headers {}",
                DisplayIter::new(deploy_hashes.iter())
            ),
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
            StorageRequest::PutChainspec { chainspec, .. } => {
                write!(
                    formatter,
                    "put chainspec {}",
                    chainspec.protocol_config.version
                )
            }
            StorageRequest::GetChainspec { version, .. } => {
                write!(formatter, "get chainspec {}", version)
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
        }
    }
}

/// State store request.
#[derive(DataSize, Debug, Serialize)]
pub enum StateStoreRequest {
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
                write!(f, "save data under {} ({} bytes)", HexFmt(key), data.len())
            }
            StateStoreRequest::Load { key, .. } => {
                write!(f, "load data from key {}", HexFmt(key))
            }
        }
    }
}

/// Details of a request for a list of deploys to propose in a new block.
#[derive(DataSize, Debug)]
pub struct ProtoBlockRequest {
    /// The instant for which the deploy is requested.
    pub(crate) current_instant: Timestamp,
    /// Set of deploy hashes of deploys that should be excluded in addition to the finalized ones.
    pub(crate) past_deploys: HashSet<DeployHash>,
    /// The height of the next block to be finalized at the point the request was made.
    /// This is _only_ a way of expressing how many blocks have been finalized at the moment the
    /// request was made. Block Proposer uses this in order to determine if there might be any
    /// deploys that are neither in `past_deploys`, nor among the finalized deploys it knows of.
    pub(crate) next_finalized: u64,
    /// Random bit with which to construct the `ProtoBlock` requested.
    pub(crate) random_bit: bool,
    /// Responder to call with the result.
    pub(crate) responder: Responder<ProtoBlock>,
}

/// A `BlockProposer` request.
#[derive(DataSize, Debug)]
#[must_use]
pub enum BlockProposerRequest {
    /// Request a list of deploys to propose in a new block.
    RequestProtoBlock(ProtoBlockRequest),
}

impl Display for BlockProposerRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockProposerRequest::RequestProtoBlock(ProtoBlockRequest {
                current_instant,
                past_deploys,
                next_finalized,
                responder: _,
                random_bit: _,
            }) => write!(
                formatter,
                "list for inclusion: instant {} past {} next_finalized {}",
                current_instant,
                past_deploys.len(),
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
pub enum RpcRequest<I> {
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
        responder: Responder<Option<(LinearBlock, BlockSignatures)>>,
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
    /// Query the contract runtime for protocol version data.
    QueryProtocolData {
        /// The protocol version.
        protocol_version: ProtocolVersion,
        /// Responder to call with the result.
        responder: Responder<Result<Option<Box<ProtocolData>>, engine_state::Error>>,
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
    /// Return string formatted, prometheus compatible metrics or `None` if an error occurred.
    GetMetrics {
        /// Responder to call with the result.
        responder: Responder<Option<String>>,
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
            RpcRequest::QueryProtocolData {
                protocol_version, ..
            } => write!(formatter, "protocol_version {}", protocol_version),
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
            RpcRequest::GetMetrics { .. } => write!(formatter, "get metrics"),
        }
    }
}

/// Abstract REST request.
///
/// An REST request is an abstract request that does not concern itself with serialization or
/// transport.
#[derive(Debug)]
#[must_use]
pub enum RestRequest<I> {
    /// Return string formatted status or `None` if an error occurred.
    GetStatus {
        /// Responder to call with the result.
        responder: Responder<StatusFeed<I>>,
    },
    /// Return string formatted, prometheus compatible metrics or `None` if an error occurred.
    GetMetrics {
        /// Responder to call with the result.
        responder: Responder<Option<String>>,
    },
}

impl<I> Display for RestRequest<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RestRequest::GetStatus { .. } => write!(formatter, "get status"),
            RestRequest::GetMetrics { .. } => write!(formatter, "get metrics"),
        }
    }
}

/// A contract runtime request.
#[derive(Debug, Serialize)]
#[must_use]
pub enum ContractRuntimeRequest {
    /// Get `ProtocolData` by `ProtocolVersion`.
    GetProtocolData {
        /// The protocol version.
        protocol_version: ProtocolVersion,
        /// Responder to call with the result.
        responder: Responder<Result<Option<Box<ProtocolData>>, engine_state::Error>>,
    },
    /// Commit genesis chainspec.
    CommitGenesis {
        /// The chainspec.
        chainspec: Box<Chainspec>,
        /// Responder to call with the result.
        responder: Responder<Result<GenesisResult, engine_state::Error>>,
    },
    /// An `ExecuteRequest` that contains multiple deploys that will be executed.
    Execute {
        /// Execution request containing deploys.
        #[serde(skip_serializing)]
        execute_request: Box<ExecuteRequest>,
        /// Responder to call with the execution result.
        responder: Responder<Result<ExecutionResults, engine_state::RootNotFound>>,
    },
    /// A request to commit existing execution transforms.
    Commit {
        /// A valid state root hash.
        state_root_hash: Digest,
        /// Effects obtained through `ExecutionResult`
        #[serde(skip_serializing)]
        effects: AdditiveMap<Key, Transform>,
        /// Responder to call with the commit result.
        responder: Responder<Result<CommitResult, engine_state::Error>>,
    },
    /// A request to run upgrade.
    Upgrade {
        /// Upgrade config.
        #[serde(skip_serializing)]
        upgrade_config: Box<UpgradeConfig>,
        /// Responder to call with the upgrade result.
        responder: Responder<Result<UpgradeResult, engine_state::Error>>,
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
    /// Returns validator weights for given era.
    GetValidatorWeightsByEraId {
        /// Get validators weights request.
        #[serde(skip_serializing)]
        request: ValidatorWeightsByEraIdRequest,
        /// Responder to call with the result.
        responder: Responder<Result<Option<ValidatorWeights>, GetEraValidatorsError>>,
    },
    /// Performs a step consisting of calculating rewards, slashing and running the auction at the
    /// end of an era.
    Step {
        /// The step request.
        #[serde(skip_serializing)]
        step_request: StepRequest,
        /// Responder to call with the result.
        responder: Responder<Result<StepResult, engine_state::Error>>,
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
        /// Responder to call with the result.
        responder: Responder<Result<InsertedTrieKeyAndMissingDescendants, engine_state::Error>>,
    },
    /// Get the missing keys under a given trie key in global storage
    MissingTrieKeys {
        /// The ancestral hash to use when finding hashes that are missing from the `TrieStore`
        trie_key: Blake2bHash,
        /// Responder to call with the result.
        responder: Responder<Result<Vec<Blake2bHash>, engine_state::Error>>,
    },
}

impl Display for ContractRuntimeRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeRequest::CommitGenesis { chainspec, .. } => {
                write!(
                    formatter,
                    "commit genesis {}",
                    chainspec.protocol_config.version
                )
            }
            ContractRuntimeRequest::Execute {
                execute_request, ..
            } => write!(
                formatter,
                "execute request: {}",
                execute_request.parent_state_hash
            ),

            ContractRuntimeRequest::Commit {
                state_root_hash,
                effects,
                ..
            } => write!(
                formatter,
                "commit request: {} {:?}",
                state_root_hash, effects
            ),

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

            ContractRuntimeRequest::GetValidatorWeightsByEraId { request, .. } => {
                write!(formatter, "get validator weights: {:?}", request)
            }

            ContractRuntimeRequest::Step { step_request, .. } => {
                write!(formatter, "step: {:?}", step_request)
            }

            ContractRuntimeRequest::GetProtocolData {
                protocol_version, ..
            } => write!(formatter, "protocol_version: {}", protocol_version),

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
            ContractRuntimeRequest::MissingTrieKeys { trie_key, .. } => {
                write!(
                    formatter,
                    "find missing descendants of trie_key: {}",
                    trie_key
                )
            }
        }
    }
}

/// Fetcher related requests.
#[derive(Debug, Serialize)]
#[must_use]
pub enum FetcherRequest<I, T: Item> {
    /// Return the specified item if it exists, else `None`.
    Fetch {
        /// The ID of the item to be retrieved.
        id: T::Id,
        /// The peer id of the peer to be asked if the item is not held locally
        peer: I,
        /// Responder to call with the result.
        responder: Responder<Option<FetchResult<T, I>>>,
    },
}

impl<I, T: Item> Display for FetcherRequest<I, T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            FetcherRequest::Fetch { id, .. } => write!(formatter, "request item by id {}", id),
        }
    }
}

/// A contract runtime request.
#[derive(Debug)]
#[must_use]
pub enum BlockExecutorRequest {
    /// A request to execute finalized block.
    ExecuteBlock(FinalizedBlock),
}

impl Display for BlockExecutorRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockExecutorRequest::ExecuteBlock(finalized_block) => {
                write!(f, "execute block {}", finalized_block)
            }
        }
    }
}

/// A block validator request.
#[derive(Debug)]
#[must_use]
pub struct BlockValidationRequest<T, I> {
    /// The block to be validated.
    pub(crate) block: T,
    /// The sender of the block, which will be asked to provide all missing deploys.
    pub(crate) sender: I,
    /// Responder to call with the result.
    ///
    /// Indicates whether or not validation was successful and returns `block` unchanged.
    pub(crate) responder: Responder<(bool, T)>,
    /// A check will be performed against the deploys to ensure their timestamp is
    /// older than or equal to the block itself.
    pub(crate) block_timestamp: Timestamp,
}

impl<T: Display, I: Display> Display for BlockValidationRequest<T, I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let BlockValidationRequest { block, sender, .. } = self;
        write!(f, "validate block {} from {}", block, sender)
    }
}

type BlockHeight = u64;

#[derive(Debug, Serialize)]
/// Requests issued to the Linear Chain component.
pub enum LinearChainRequest<I> {
    /// Request whole block from the linear chain, by hash.
    BlockRequest(BlockHash, I),
    /// Request for a linear chain block at height.
    BlockAtHeight(BlockHeight, I),
    /// Local request for a linear chain block at height.
    /// TODO: Unify `BlockAtHeight` and `BlockAtHeightLocal`.
    BlockAtHeightLocal(BlockHeight, Responder<Option<Block>>),
}

impl<I: Display> Display for LinearChainRequest<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LinearChainRequest::BlockRequest(bh, peer) => {
                write!(f, "block request for hash {} from {}", bh, peer)
            }
            LinearChainRequest::BlockAtHeight(height, sender) => {
                write!(f, "block request for {} from {}", height, sender)
            }
            LinearChainRequest::BlockAtHeightLocal(height, _) => {
                write!(f, "local request for block at height {}", height)
            }
        }
    }
}

#[derive(DataSize, Debug)]
#[must_use]
/// Consensus component requests.
pub enum ConsensusRequest {
    /// Request for consensus to sign a new linear chain block and possibly start a new era.
    HandleLinearBlock(Box<BlockHeader>, Responder<Option<FinalitySignature>>),
    /// Check whether validator identifying with the public key is bonded.
    IsBondedValidator(EraId, PublicKey, Responder<bool>),
}

/// ChainspecLoader component requests.
#[derive(Debug, Serialize)]
pub enum ChainspecLoaderRequest {
    /// Chainspec info request.
    GetChainspecInfo(Responder<ChainspecInfo>),
}

impl Display for ChainspecLoaderRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainspecLoaderRequest::GetChainspecInfo(_) => write!(f, "get chainspec info"),
        }
    }
}
