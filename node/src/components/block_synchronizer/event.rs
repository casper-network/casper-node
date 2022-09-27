use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use derive_more::From;
use serde::Serialize;

use casper_types::{EraId, PublicKey, U512};

use super::GlobalStateSynchronizerEvent;
use crate::{
    components::{block_synchronizer::BlockSyncRequest, fetcher::FetchResult},
    types::{
        BlockAdded, BlockHash, BlockHeader, Deploy, EraValidatorWeights, FinalitySignature, NodeId,
        TrieOrChunk, TrieOrChunkId,
    },
};

#[derive(From, Debug, Serialize)]
pub(crate) enum Event {
    /// The initiating event to fetch an item by its id.
    #[from]
    Upsert(BlockSyncRequest),

    /// Received announcement about era validators.
    EraValidators {
        era_validator_weights: EraValidatorWeights,
    },

    Next,

    DisconnectFromPeer(NodeId),

    #[from]
    BlockHeaderFetched(FetchResult<BlockHeader>),
    #[from]
    BlockAddedFetched(FetchResult<BlockAdded>),
    #[from]
    FinalitySignatureFetched(FetchResult<FinalitySignature>),
    DeployFetched {
        block_hash: BlockHash,
        result: FetchResult<Deploy>,
    },

    #[from]
    GlobalStateSynchronizer(GlobalStateSynchronizerEvent),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Upsert(request) => {
                write!(f, "upsert: {}", request)
            }
            Event::EraValidators { .. } => {
                write!(f, "new era validators")
            }
            Event::Next => {
                write!(f, "next")
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
        }
    }
}
