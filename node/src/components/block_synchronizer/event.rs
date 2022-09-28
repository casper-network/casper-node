use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use casper_hashing::Digest;
use derive_more::From;
use serde::Serialize;

use casper_types::{EraId, PublicKey, U512};

use super::GlobalStateSynchronizerEvent;
use crate::components::block_synchronizer::GlobalStateSynchronizerError;
use crate::effect::requests::BlockSynchronizerRequest;
use crate::{
    components::fetcher::FetchResult,
    types::{
        BlockAdded, BlockHash, BlockHeader, Deploy, EraValidatorWeights, FinalitySignature, NodeId,
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
    BlockAddedFetched(FetchResult<BlockAdded>),
    #[from]
    FinalitySignatureFetched(FetchResult<FinalitySignature>),
    GlobalStateSynced {
        block_hash: BlockHash,
        #[serde(skip_serializing)]
        result: Result<Digest, GlobalStateSynchronizerError>,
    },
    DeployFetched {
        block_hash: BlockHash,
        result: FetchResult<Deploy>,
    },
    AccumulatedPeers(BlockHash, Option<Vec<NodeId>>),
    #[from]
    GlobalStateSynchronizer(GlobalStateSynchronizerEvent),
    // ExecutionResultsFetched {
    //     block_hash: BlockHash,
    //     result: FetchResult<ExecutionResults>,
    // },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(BlockSynchronizerRequest::NeedNext { .. }) => {
                write!(
                    f,
                    "block synchronizer need next request"
                )
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
            Event::DeployFetched {
                block_hash: _,
                result,
            } => match result {
                Ok(fetched_item) => write!(f, "{}", fetched_item),
                Err(fetcher_error) => write!(f, "{}", fetcher_error),
            },
            Event::GlobalStateSynchronizer(event) => {
                write!(f, "{:?}", event)
            }
            Event::AccumulatedPeers(..) => {
                write!(f, "accumulated peers")
            }
            // Event::ExecutionResultsFetched {
            //     block_hash: _,
            //     result,
            // } => match result {
            //     Ok(fetched_item) => write!(f, "{}", fetched_item),
            //     Err(fetcher_error) => write!(f, "{}", fetcher_error),
            // },
        }
    }
}
