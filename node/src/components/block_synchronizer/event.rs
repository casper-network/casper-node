use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use derive_more::From;
use serde::Serialize;

use casper_types::{EraId, PublicKey, U512};

use crate::{
    components::{block_synchronizer::BlockSyncRequest, fetcher::FetchResult},
    types::{BlockAdded, BlockHash, Deploy, FinalitySignature, NodeId, TrieOrChunk, TrieOrChunkId},
};

#[derive(From, Debug, Serialize)]
pub(crate) enum Event {
    /// The initiating event to fetch an item by its id.
    #[from]
    Upsert(BlockSyncRequest),

    /// Received announcement about upcoming era validators.
    EraValidators {
        validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    },

    Next,

    DisconnectFromPeer(NodeId),

    #[from]
    BlockAddedFetched(FetchResult<BlockAdded>),
    #[from]
    FinalitySignatureFetched(FetchResult<FinalitySignature>),
    #[from]
    DeployFetched(FetchResult<Deploy>),
    TrieOrChunkFetched {
        block_hash: BlockHash,
        id: TrieOrChunkId,
        fetch_result: FetchResult<TrieOrChunk>,
    },
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
            Event::DeployFetched(Ok(fetched_item)) => {
                write!(f, "{}", fetched_item)
            }
            Event::DeployFetched(Err(fetcher_error)) => {
                write!(f, "{}", fetcher_error)
            }
            Event::TrieOrChunkFetched {
                block_hash,
                id,
                fetch_result,
            } => match fetch_result {
                Ok(fetched_item) => {
                    write!(f, "fetching {} for {}, {}", id, block_hash, fetched_item)
                }
                Err(fetcher_error) => {
                    write!(f, "fetching {} for {}, {}", id, block_hash, fetcher_error)
                }
            },
        }
    }
}
