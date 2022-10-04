use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use casper_hashing::Digest;
use derive_more::From;
use either::Either;
use serde::Serialize;
use tracing::event;

use casper_execution_engine::core::engine_state;
use casper_types::{EraId, PublicKey, U512};

use super::GlobalStateSynchronizerEvent;
use crate::{
    components::{block_synchronizer::GlobalStateSynchronizerError, fetcher::FetchResult},
    effect::requests::BlockSynchronizerRequest,
    types::{
        ExecutedBlock, BlockExecutionResultsOrChunk, BlockHash, BlockHeader, Deploy,
        EraValidatorWeights, FinalitySignature, LegacyDeploy, NodeId,
    },
};

#[derive(From, Debug, Serialize)]
pub(crate) enum Event {
    #[from]
    Request(BlockSynchronizerRequest),
    /// Received announcement about era validators.
    EraValidators {
        era_validator_weights: EraValidatorWeights,
    },

    MaybeEraValidators(EraId, Option<BTreeMap<PublicKey, U512>>),

    DisconnectFromPeer(NodeId),

    #[from]
    BlockHeaderFetched(FetchResult<BlockHeader>),
    #[from]
    BlockAddedFetched(FetchResult<ExecutedBlock>),
    #[from]
    FinalitySignatureFetched(FetchResult<FinalitySignature>),
    GlobalStateSynced {
        block_hash: BlockHash,
        #[serde(skip_serializing)]
        result: Result<Digest, GlobalStateSynchronizerError>,
    },
    GotExecutionResultsRootHash {
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
    ExecutionResultsStored(BlockHash),
    AccumulatedPeers(BlockHash, Option<Vec<NodeId>>),
    #[from]
    GlobalStateSynchronizer(GlobalStateSynchronizerEvent),
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
            Event::EraValidators { .. } => {
                write!(f, "new era validators")
            }
            Event::MaybeEraValidators(era_id, _) => {
                write!(f, "maybe new new era validators for era_id: {}", era_id)
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
            Event::BlockAddedFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::BlockAddedFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::FinalitySignatureFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::FinalitySignatureFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::GlobalStateSynced {
                block_hash: _,
                result,
            } => match result {
                Ok(root_hash) => write!(f, "synced global state under root {}", root_hash),
                Err(error) => write!(f, "failed to sync global state: {}", error),
            },
            Event::GotExecutionResultsRootHash {
                block_hash: _,
                result,
            } => match result {
                Ok(Some(digest)) => write!(f, "got exec results root hash {}", digest),
                Ok(None) => write!(f, "got no exec results root hash"),
                Err(error) => write!(f, "failed to get exec results root hash: {}", error),
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
            Event::GlobalStateSynchronizer(event) => {
                write!(f, "{:?}", event)
            }
            Event::AccumulatedPeers(..) => {
                write!(f, "accumulated peers")
            }
        }
    }
}
