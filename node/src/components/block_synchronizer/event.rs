use std::fmt::{self, Display, Formatter};

use thiserror::Error;

use casper_hashing::Digest;
use casper_types::system::auction::EraValidators;
use derive_more::From;
use either::Either;
use serde::Serialize;

use casper_execution_engine::{
    core::engine_state::{self, GetEraValidatorsError},
    storage::trie::TrieRaw,
};

use crate::{
    components::fetcher::FetchResult,
    effect::requests::BlockSynchronizerRequest,
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash, BlockHeader, Deploy,
        FinalitySignature, FinalizedBlock, LegacyDeploy, NodeId, SyncLeap, TrieOrChunk,
    },
};

#[derive(Debug, Error, Serialize)]
pub(crate) enum EraValidatorsGetError {
    /// Invalid state hash was used to make this request
    #[error("Invalid state hash")]
    RootNotFound,
    /// Engine state error
    #[error("Engine state error")]
    Other,
    /// EraValidators missing
    #[error("Era validators missing")]
    EraValidatorsMissing,
    /// Unexpected query failure.
    #[error("Unexpected query failure")]
    UnexpectedQueryFailure,
    /// CLValue conversion error.
    #[error("CLValue conversion error")]
    CLValue,
}

impl From<GetEraValidatorsError> for EraValidatorsGetError {
    fn from(get_era_validators_error: GetEraValidatorsError) -> Self {
        match get_era_validators_error {
            GetEraValidatorsError::RootNotFound => EraValidatorsGetError::RootNotFound,
            GetEraValidatorsError::Other(_) => EraValidatorsGetError::Other,
            GetEraValidatorsError::EraValidatorsMissing => {
                EraValidatorsGetError::EraValidatorsMissing
            }
            GetEraValidatorsError::UnexpectedQueryFailure => {
                EraValidatorsGetError::UnexpectedQueryFailure
            }
            GetEraValidatorsError::CLValue => EraValidatorsGetError::CLValue,
        }
    }
}

#[derive(From, Debug, Serialize)]
pub(crate) enum Event {
    Initialize,
    #[from]
    Request(BlockSynchronizerRequest),
    DisconnectFromPeer(NodeId),
    #[from]
    MadeFinalizedBlock {
        block_hash: BlockHash,
        result: Option<(FinalizedBlock, Vec<Deploy>)>,
    },
    MarkBlockExecutionEnqueued(BlockHash),
    MarkBlockExecuted(BlockHash),
    MarkBlockCompleted {
        block_hash: BlockHash,
        is_new: bool,
    },
    #[from]
    BlockHeaderFetched(FetchResult<BlockHeader>),
    #[from]
    BlockFetched(FetchResult<Block>),
    #[from]
    ApprovalsHashesFetched(FetchResult<ApprovalsHashes>),
    #[from]
    FinalitySignatureFetched(FetchResult<FinalitySignature>),
    #[from]
    SyncLeapFetched(FetchResult<SyncLeap>),
    GotExecutionResultsChecksum {
        block_hash: BlockHash,
        #[serde(skip_serializing)]
        result: Result<Option<Digest>, engine_state::Error>,
    },
    DeployFetched {
        block_hash: BlockHash,
        result: Either<FetchResult<LegacyDeploy>, FetchResult<Deploy>>,
    },
    ExecutionResultsFetched {
        block_hash: BlockHash,
        result: FetchResult<BlockExecutionResultsOrChunk>,
    },
    TrieOrChunkFetched {
        block_hash: BlockHash,
        state_root_hash: Digest,
        trie_hash: Digest,
        result: FetchResult<TrieOrChunk>,
    },
    PutTrieResult {
        state_root_hash: Digest,
        trie_hash: Digest,
        trie_raw: Box<TrieRaw>,
        #[serde(skip)]
        put_trie_result: Result<Digest, engine_state::Error>,
    },
    ExecutionResultsStored(BlockHash),
    AccumulatedPeers(BlockHash, Option<Vec<NodeId>>),
    NetworkPeers(BlockHash, Vec<NodeId>),
    EraValidatorsFromContractRuntime(Digest, Result<EraValidators, EraValidatorsGetError>),
    BlockHeaderFromStorage(BlockHash, Option<BlockHeader>),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(BlockSynchronizerRequest::NeedNext { .. }) => {
                write!(f, "block synchronizer need next request")
            }
            Event::Request(_) => {
                write!(f, "block synchronizer request from effect builder")
            }
            Event::Initialize => {
                write!(f, "initialize this component")
            }
            Event::DisconnectFromPeer(peer) => {
                write!(f, "disconnected from peer {}", peer)
            }
            Event::BlockHeaderFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::BlockHeaderFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::BlockFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::BlockFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::ApprovalsHashesFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::ApprovalsHashesFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::FinalitySignatureFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::FinalitySignatureFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::SyncLeapFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::SyncLeapFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::GotExecutionResultsChecksum {
                block_hash: _,
                result,
            } => match result {
                Ok(Some(digest)) => write!(f, "got exec results checksum {}", digest),
                Ok(None) => write!(f, "got no exec results checksum"),
                Err(error) => write!(f, "failed to get exec results checksum: {}", error),
            },
            Event::DeployFetched {
                block_hash: _,
                result,
            } => match result {
                Either::Left(Ok(fetched_item)) => write!(f, "{}", fetched_item),
                Either::Left(Err(fetcher_error)) => write!(f, "{}", fetcher_error),
                Either::Right(Ok(fetched_item)) => write!(f, "{}", fetched_item),
                Either::Right(Err(fetcher_error)) => write!(f, "{}", fetcher_error),
            },
            Event::ExecutionResultsFetched {
                block_hash: _,
                result,
            } => match result {
                Ok(fetched_item) => write!(f, "{}", fetched_item),
                Err(fetcher_error) => write!(f, "{}", fetcher_error),
            },
            Event::ExecutionResultsStored { .. } => write!(f, "stored execution results"),
            Event::NetworkPeers(..) => {
                write!(f, "network peers")
            }
            Event::AccumulatedPeers(..) => {
                write!(f, "accumulated peers")
            }
            Event::MadeFinalizedBlock { .. } => {
                write!(f, "made finalized block")
            }
            Event::MarkBlockExecutionEnqueued(..) => {
                write!(f, "mark block enqueued for execution")
            }
            Event::MarkBlockExecuted(..) => {
                write!(f, "block execution complete")
            }
            Event::MarkBlockCompleted { .. } => {
                write!(f, "mark block completed")
            }
            Event::TrieOrChunkFetched {
                block_hash,
                state_root_hash,
                trie_hash,
                result: _,
            } => {
                write!(
                    f,
                    "fetch response received syncing trie hash {} block {} with state root hash {}",
                    trie_hash, block_hash, state_root_hash
                )
            }
            Event::PutTrieResult {
                state_root_hash,
                trie_hash,
                trie_raw: _,
                put_trie_result: _,
            } => {
                write!(
                    f,
                    "put trie result for trie {} acquiring state root hash {}",
                    trie_hash, state_root_hash
                )
            }
            Event::EraValidatorsFromContractRuntime(root_hash, _) => {
                write!(
                    f,
                    "era validators returned from contract runtime for root hash {}",
                    root_hash
                )
            }
            Event::BlockHeaderFromStorage(block_hash, _) => {
                write!(
                    f,
                    "block header from storage response required for syncing block {}",
                    block_hash
                )
            }
        }
    }
}
