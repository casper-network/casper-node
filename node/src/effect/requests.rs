//! Request effects.
//!
//! Requests typically ask other components to perform a service and report back the result. See the
//! top-level module documentation for details.

use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    net::SocketAddr,
};

use datasize::DataSize;
use semver::Version;

use casper_execution_engine::{
    core::engine_state::{
        self,
        balance::{BalanceRequest, BalanceResult},
        era_validators::{GetEraValidatorsError, GetEraValidatorsRequest},
        execute_request::ExecuteRequest,
        execution_result::ExecutionResults,
        genesis::GenesisResult,
        query::{QueryRequest, QueryResult},
        step::{StepRequest, StepResult},
        upgrade::{UpgradeConfig, UpgradeResult},
    },
    shared::{additive_map::AdditiveMap, transform::Transform},
    storage::global_state::CommitResult,
};
use casper_types::{auction::ValidatorWeights, Key, URef};

use super::Responder;
use crate::{
    components::{
        chainspec_loader::ChainspecInfo,
        fetcher::FetchResult,
        storage::{
            DeployHashes, DeployHeaderResults, DeployMetadata, DeployResults, StorageType, Value,
        },
    },
    crypto::{asymmetric_key::Signature, hash::Digest},
    types::{
        json_compatibility::ExecutionResult, Block as LinearBlock, BlockHash, BlockHeader, Deploy,
        DeployHash, FinalizedBlock, Item, ProtoBlockHash, StatusFeed, Timestamp,
    },
    utils::DisplayIter,
    Chainspec,
};

type DeployAndMetadata<S> = (
    <S as StorageType>::Deploy,
    DeployMetadata<<S as StorageType>::Block>,
);

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
#[derive(Debug)]
#[must_use]
pub enum NetworkRequest<I, P> {
    /// Send a message on the network to a specific peer.
    SendMessage {
        /// Message destination.
        dest: I,
        /// Message payload.
        payload: P,
        /// Responder to be called when the message is queued.
        responder: Responder<()>,
    },
    /// Send a message on the network to all peers.
    /// Note: This request is deprecated and should be phased out, as not every network
    ///       implementation is likely to implement broadcast support.
    Broadcast {
        /// Message payload.
        payload: P,
        /// Responder to be called when all messages are queued.
        responder: Responder<()>,
    },
    /// Gossip a message to a random subset of peers.
    Gossip {
        /// Payload to gossip.
        payload: P,
        /// Number of peers to gossip to. This is an upper bound, otherwise best-effort.
        count: usize,
        /// Node IDs of nodes to exclude from gossiping to.
        exclude: HashSet<I>,
        /// Responder to be called when all messages are queued.
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
        responder: Responder<HashMap<I, SocketAddr>>,
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

#[derive(Debug)]
/// A storage request.
#[must_use]
pub enum StorageRequest<S: StorageType + 'static> {
    /// Store given block.
    PutBlock {
        /// Block to be stored.
        block: Box<S::Block>,
        /// Responder to call with the result.  Returns true if the block was stored on this
        /// attempt or false if it was previously stored.
        responder: Responder<bool>,
    },
    /// Retrieve block with given hash.
    GetBlock {
        /// Hash of block to be retrieved.
        block_hash: <S::Block as Value>::Id,
        /// Responder to call with the result.  Returns `None` is the block doesn't exist in local
        /// storage.
        responder: Responder<Option<S::Block>>,
    },
    /// Retrieve block with given height.
    GetBlockAtHeight {
        /// Height of the block.
        height: u64,
        /// Responder.
        responder: Responder<Option<S::Block>>,
    },
    /// Retrieve highest block.
    GetHighestBlock {
        /// Responder.
        responder: Responder<Option<S::Block>>,
    },
    /// Retrieve block header with given hash.
    GetBlockHeader {
        /// Hash of block to get header of.
        block_hash: <S::Block as Value>::Id,
        /// Responder to call with the result.  Returns `None` is the block header doesn't exist in
        /// local storage.
        responder: Responder<Option<<S::Block as Value>::Header>>,
    },
    /// Store given deploy.
    PutDeploy {
        /// Deploy to store.
        deploy: Box<S::Deploy>,
        /// Responder to call with the result.  Returns true if the deploy was stored on this
        /// attempt or false if it was previously stored.
        responder: Responder<bool>,
    },
    /// Retrieve deploys with given hashes.
    GetDeploys {
        /// Hashes of deploys to be retrieved.
        deploy_hashes: DeployHashes<S>,
        /// Responder to call with the results.
        responder: Responder<DeployResults<S>>,
    },
    /// Retrieve deploy headers with given hashes.
    GetDeployHeaders {
        /// Hashes of deploy headers to be retrieved.
        deploy_hashes: DeployHashes<S>,
        /// Responder to call with the results.
        responder: Responder<DeployHeaderResults<S>>,
    },
    /// Store the given execution results for the deploys in the given block.
    PutExecutionResults {
        /// Hash of block.
        block_hash: <S::Block as Value>::Id,
        /// Execution results.
        execution_results: HashMap<<S::Deploy as Value>::Id, ExecutionResult>,
        /// Responder to call with the result.  Returns true if the execution results were stored
        /// on this attempt or false if they were previously stored.
        responder: Responder<()>,
    },
    /// Retrieve deploy and its metadata.
    GetDeployAndMetadata {
        /// Hash of deploy to be retrieved.
        deploy_hash: <S::Deploy as Value>::Id,
        /// Responder to call with the results.
        responder: Responder<Option<DeployAndMetadata<S>>>,
    },
    /// Store given chainspec.
    PutChainspec {
        /// Chainspec.
        chainspec: Box<Chainspec>,
        /// Responder to call with the result.
        responder: Responder<()>,
    },
    /// Retrieve chainspec with given version.
    GetChainspec {
        /// Version.
        version: Version,
        /// Responder to call with the result.
        responder: Responder<Option<Chainspec>>,
    },
}

impl<S: StorageType> Display for StorageRequest<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, .. } => write!(formatter, "put {}", block),
            StorageRequest::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
            StorageRequest::GetBlockAtHeight { height, .. } => {
                write!(formatter, "get block at height {}", height)
            }
            StorageRequest::GetHighestBlock { .. } => write!(formatter, "get highest block"),
            StorageRequest::GetBlockHeader { block_hash, .. } => {
                write!(formatter, "get {}", block_hash)
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
            StorageRequest::PutChainspec { chainspec, .. } => write!(
                formatter,
                "put chainspec {}",
                chainspec.genesis.protocol_version
            ),
            StorageRequest::GetChainspec { version, .. } => {
                write!(formatter, "get chainspec {}", version)
            }
        }
    }
}

/// A `DeployBuffer` request.
#[derive(Debug)]
#[must_use]
pub enum DeployBufferRequest {
    /// Request a list of deploys to propose in a new block.
    ListForInclusion {
        /// The instant for which the deploy is requested.
        current_instant: Timestamp,
        /// Set of block hashes pointing to blocks whose deploys should be excluded.
        past_blocks: HashSet<ProtoBlockHash>,
        /// Responder to call with the result.
        responder: Responder<HashSet<DeployHash>>,
    },
}

impl Display for DeployBufferRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DeployBufferRequest::ListForInclusion {
                current_instant,
                past_blocks,
                responder: _,
            } => write!(
                formatter,
                "list for inclusion: instant {} past {}",
                current_instant,
                past_blocks.len()
            ),
        }
    }
}

/// Abstract API request.
///
/// An API request is an abstract request that does not concern itself with serialization or
/// transport.
#[derive(Debug)]
#[must_use]
pub enum ApiRequest<I> {
    /// Submit a deploy to be announced.
    SubmitDeploy {
        /// The deploy to be announced.
        deploy: Box<Deploy>,
        /// Responder to call.
        responder: Responder<()>,
    },
    /// If `maybe_hash` is `Some`, return the specified block if it exists, else `None`.  If
    /// `maybe_hash` is `None`, return the latest block.
    GetBlock {
        /// The hash of the block to be retrieved.
        maybe_hash: Option<BlockHash>,
        /// Responder to call with the result.
        responder: Responder<Option<LinearBlock>>,
    },
    /// Query the global state at the given root hash.
    QueryGlobalState {
        /// The global state hash.
        global_state_hash: Digest,
        /// Hex-encoded `casper_types::Key`.
        base_key: Key,
        /// The path components starting from the key as base.
        path: Vec<String>,
        /// Responder to call with the result.
        responder: Responder<Result<QueryResult, engine_state::Error>>,
    },
    /// Query the global state at the given root hash.
    GetBalance {
        /// The global state hash.
        global_state_hash: Digest,
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
        responder: Responder<Option<(Deploy, DeployMetadata<LinearBlock>)>>,
    },
    /// Return the connected peers.
    GetPeers {
        /// Responder to call with the result.
        responder: Responder<HashMap<I, SocketAddr>>,
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

impl<I> Display for ApiRequest<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ApiRequest::SubmitDeploy { deploy, .. } => write!(formatter, "submit {}", *deploy),
            ApiRequest::GetBlock {
                maybe_hash: Some(hash),
                ..
            } => write!(formatter, "get {}", hash),
            ApiRequest::GetBlock {
                maybe_hash: None, ..
            } => write!(formatter, "get latest block"),
            ApiRequest::QueryGlobalState {
                global_state_hash,
                base_key,
                path,
                ..
            } => write!(
                formatter,
                "query {}, base_key: {}, path: {:?}",
                global_state_hash, base_key, path
            ),
            ApiRequest::GetBalance {
                global_state_hash,
                purse_uref,
                ..
            } => write!(
                formatter,
                "balance {}, purse_uref: {}",
                global_state_hash, purse_uref
            ),
            ApiRequest::GetDeploy { hash, .. } => write!(formatter, "get {}", hash),
            ApiRequest::GetPeers { .. } => write!(formatter, "get peers"),
            ApiRequest::GetStatus { .. } => write!(formatter, "get status"),
            ApiRequest::GetMetrics { .. } => write!(formatter, "get metrics"),
        }
    }
}

/// A contract runtime request.
#[derive(Debug)]
#[must_use]
pub enum ContractRuntimeRequest {
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
        execute_request: ExecuteRequest,
        /// Responder to call with the execution result.
        responder: Responder<Result<ExecutionResults, engine_state::RootNotFound>>,
    },
    /// A request to commit existing execution transforms.
    Commit {
        /// A valid pre state hash.
        pre_state_hash: Digest,
        /// Effects obtained through `ExecutionResult`
        effects: AdditiveMap<Key, Transform>,
        /// Responder to call with the commit result.
        responder: Responder<Result<CommitResult, engine_state::Error>>,
    },
    /// A request to run upgrade.
    Upgrade {
        /// Upgrade config.
        upgrade_config: UpgradeConfig,
        /// Responder to call with the upgrade result.
        responder: Responder<Result<UpgradeResult, engine_state::Error>>,
    },
    /// A query request.
    Query {
        /// Query request.
        query_request: QueryRequest,
        /// Responder to call with the query result.
        responder: Responder<Result<QueryResult, engine_state::Error>>,
    },
    /// A balance request.
    GetBalance {
        /// Balance request.
        balance_request: BalanceRequest,
        /// Responder to call with the balance result.
        responder: Responder<Result<BalanceResult, engine_state::Error>>,
    },
    /// Returns validator weights for given era.
    GetEraValidators {
        /// Get era validators request.
        get_request: GetEraValidatorsRequest,
        /// Responder to call with the result.
        responder: Responder<Result<Option<ValidatorWeights>, GetEraValidatorsError>>,
    },
    /// Performs a step consisting of calculating rewards, slashing and running the auction at the
    /// end of an era.
    Step {
        /// The step request.
        step_request: StepRequest,
        /// Responder to call with the result.
        responder: Responder<Result<StepResult, engine_state::Error>>,
    },
}

impl Display for ContractRuntimeRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeRequest::CommitGenesis { chainspec, .. } => write!(
                formatter,
                "commit genesis {}",
                chainspec.genesis.protocol_version
            ),
            ContractRuntimeRequest::Execute {
                execute_request, ..
            } => write!(
                formatter,
                "execute request: {}",
                execute_request.parent_state_hash
            ),

            ContractRuntimeRequest::Commit {
                pre_state_hash,
                effects,
                ..
            } => write!(
                formatter,
                "commit request: {} {:?}",
                pre_state_hash, effects
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

            ContractRuntimeRequest::GetEraValidators { get_request, .. } => {
                write!(formatter, "get validator weights: {:?}", get_request)
            }

            ContractRuntimeRequest::Step { step_request, .. } => {
                write!(formatter, "step: {:?}", step_request)
            }
        }
    }
}

/// Fetcher related requests.
#[derive(Debug)]
#[must_use]
pub enum FetcherRequest<I, T: Item> {
    /// Return the specified item if it exists, else `None`.
    Fetch {
        /// The ID of the item to be retrieved.
        id: T::Id,
        /// The peer id of the peer to be asked if the item is not held locally
        peer: I,
        /// Responder to call with the result.
        responder: Responder<Option<FetchResult<T>>>,
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
}

impl<T: Display, I: Display> Display for BlockValidationRequest<T, I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let BlockValidationRequest { block, sender, .. } = self;
        write!(f, "validate block {} from {}", block, sender)
    }
}

type BlockHeight = u64;

#[derive(Debug)]
/// Requests issued to the Linear Chain component.
pub enum LinearChainRequest<I> {
    /// Request whole block from the linear chain, by hash.
    BlockRequest(BlockHash, I),
    /// Request for a linear chain block at height.
    BlockAtHeight(BlockHeight, I),
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
        }
    }
}

#[derive(DataSize, Debug)]
#[must_use]
/// Consensus component requests.
pub enum ConsensusRequest {
    /// Request for consensus to sign a new linear chain block and possibly start a new era.
    HandleLinearBlock(Box<BlockHeader>, Responder<Signature>),
}

/// ChainspecLoader componenent requests.
#[derive(Debug)]
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
