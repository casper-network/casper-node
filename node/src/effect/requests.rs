//! Request effects.
//!
//! Requests typically ask other components to perform a service and report back the result. See the
//! top-level module documentation for details.

use std::{
    collections::HashSet,
    fmt::{self, Debug, Display, Formatter},
};

use semver::Version;

use types::{Key, ProtocolVersion};

use super::Responder;
use crate::{
    components::{
        contract_runtime::{
            core::engine_state::{
                self,
                execute_request::ExecuteRequest,
                execution_result::ExecutionResult,
                genesis::GenesisResult,
                query::{QueryRequest, QueryResult},
                upgrade::{UpgradeConfig, UpgradeResult},
            },
            shared::{additive_map::AdditiveMap, transform::Transform},
            storage::global_state::CommitResult,
        },
        deploy_buffer::BlockLimits,
        storage::{self, DeployHashes, DeployHeaderResults, DeployResults, StorageType, Value},
    },
    crypto::hash::Digest,
    types::{BlockHash, Deploy, DeployHash, DeployHeader, ExecutedBlock, FinalizedBlock},
    utils::DisplayIter,
    Chainspec,
};

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

#[derive(Debug)]
// TODO: remove once all variants are used.
/// A storage request.
#[allow(dead_code)]
#[must_use]
pub enum StorageRequest<S: StorageType + 'static> {
    /// Store given block.
    PutBlock {
        /// Block to be stored.
        block: Box<S::Block>,
        /// Responder to call with the result.  Returns true if the block was stored on this attempt
        /// or false if it was previously stored.
        responder: Responder<storage::Result<bool>>,
    },
    /// Retrieve block with given hash.
    GetBlock {
        /// Hash of block to be retrieved.
        block_hash: <S::Block as Value>::Id,
        /// Responder to call with the result.
        responder: Responder<storage::Result<S::Block>>,
    },
    /// Retrieve block header with given hash.
    GetBlockHeader {
        /// Hash of block to get header of.
        block_hash: <S::Block as Value>::Id,
        /// Responder to call with the result.
        responder: Responder<storage::Result<<S::Block as Value>::Header>>,
    },
    /// Store given deploy.
    PutDeploy {
        /// Deploy to store.
        deploy: Box<S::Deploy>,
        /// Responder to call with the result.  Returns true if the deploy was stored on this
        /// attempt or false if it was previously stored.
        responder: Responder<storage::Result<bool>>,
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
    /// List all deploy hashes.
    ListDeploys {
        /// Responder to call with the result.
        responder: Responder<storage::Result<Vec<<S::Deploy as Value>::Id>>>,
    },
    /// Store given chainspec.
    PutChainspec {
        /// Chainspec.
        chainspec: Box<Chainspec>,
        /// Responder to call with the result.
        responder: Responder<storage::Result<()>>,
    },
    /// Retrieve chainspec with given version.
    GetChainspec {
        /// Version.
        version: Version,
        /// Responder to call with the result.
        responder: Responder<storage::Result<Chainspec>>,
    },
}

impl<S: StorageType> Display for StorageRequest<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, .. } => write!(formatter, "put {}", block),
            StorageRequest::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
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
            StorageRequest::ListDeploys { .. } => write!(formatter, "list deploys"),
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

#[allow(dead_code)] // FIXME: Remove once in use.
/// Deploy-queue related requests.
#[derive(Debug)]
#[must_use]
pub enum DeployQueueRequest {
    /// Add a deploy to the queue for inclusion into an upcoming block.
    QueueDeploy {
        /// Hash of deploy to store.
        hash: DeployHash,
        /// Header of the deploy to store.
        header: DeployHeader,
        /// Responder to call with the result.
        responder: Responder<bool>,
    },

    /// Request a list of deploys to propose in a new block.
    RequestForInclusion {
        /// The instant for which the deploy is requested.
        current_instant: u64, // TODO: timestamp: Timestamp,
        /// Maximum time to live.
        max_ttl: u32,
        /// Gas, size and count limits for deploys in a block.
        limits: BlockLimits,
        /// Maximum number of dependencies.
        max_dependencies: u8,
        /// Set of block hashes pointing to blocks whose deploys should be excluded.
        past: HashSet<BlockHash>,
        /// Responder to call with the result.
        responder: Responder<HashSet<DeployHash>>,
    },
}

impl Display for DeployQueueRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DeployQueueRequest::QueueDeploy { hash, .. } => {
                write!(formatter, "add deploy {} to queue", hash)
            }
            DeployQueueRequest::RequestForInclusion {
                current_instant,
                max_ttl,
                limits,
                max_dependencies,
                past,
                responder: _,
            } => write!(
                formatter,
                "request for inclusion: instant {} ttl {} block_size {} gas_limit {} \
                        max_deps {} max_deploy_count {} #past {}",
                current_instant,
                max_ttl,
                limits.size_bytes,
                limits.gas,
                max_dependencies,
                limits.deploy_count,
                past.len()
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
pub enum ApiRequest {
    /// Submit a deploy to be announced.
    SubmitDeploy {
        /// The deploy to be announced.
        deploy: Box<Deploy>,
        /// Responder to call.
        responder: Responder<()>,
    },
    /// Return the specified deploy if it exists, else `None`.
    GetDeploy {
        /// The hash of the deploy to be retrieved.
        hash: DeployHash,
        /// Responder to call with the result.
        responder: Responder<Result<Deploy, storage::Error>>,
    },
    /// Return the list of all deploy hashes stored on this node.
    ListDeploys {
        /// Responder to call with the result.
        responder: Responder<Result<Vec<DeployHash>, storage::Error>>,
    },
}

impl Display for ApiRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ApiRequest::SubmitDeploy { deploy, .. } => write!(formatter, "submit {}", *deploy),
            ApiRequest::GetDeploy { hash, .. } => write!(formatter, "get {}", hash),
            ApiRequest::ListDeploys { .. } => write!(formatter, "list deploys"),
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
        responder: Responder<Result<Vec<ExecutionResult>, engine_state::RootNotFound>>,
    },
    /// A request to commit existing execution transforms.
    Commit {
        /// Current protocol version of the commit request.
        protocol_version: ProtocolVersion,
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
        /// Responder to call with the upgrade result.
        responder: Responder<Result<QueryResult, engine_state::Error>>,
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
                protocol_version,
                pre_state_hash,
                effects,
                ..
            } => write!(
                formatter,
                "commit request: {} {} {:?}",
                protocol_version, pre_state_hash, effects
            ),

            ContractRuntimeRequest::Upgrade { upgrade_config, .. } => {
                write!(formatter, "upgrade request: {:?}", upgrade_config)
            }

            ContractRuntimeRequest::Query { query_request, .. } => {
                write!(formatter, "query request: {:?}", query_request)
            }
        }
    }
}

/// A contract runtime request.
#[derive(Debug)]
#[must_use]
pub enum BlockExecutorRequest {
    /// A request to execute finalized block.
    ExecuteBlock {
        /// The finalized block.
        finalized_block: FinalizedBlock,
        /// Responder to call with the result.
        responder: Responder<ExecutedBlock>,
    },
}

impl Display for BlockExecutorRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockExecutorRequest::ExecuteBlock {
                finalized_block, ..
            } => write!(f, "execute block {}", finalized_block),
        }
    }
}
