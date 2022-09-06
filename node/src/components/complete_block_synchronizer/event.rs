use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

use derive_more::From;
use serde::Serialize;

use casper_types::{EraId, PublicKey, U512};

use crate::{
    components::{complete_block_synchronizer::CompleteBlockSyncRequest, fetcher::FetchResult},
    types::{NodeId, Block, BlockHash, BlockSignatures, Deploy, DeployHash, TrieOrChunk, TrieOrChunkId},
};

#[derive(From, Debug, Serialize)]
pub(crate) enum Event {
    /// The initiating event to fetch an item by its id.
    #[from]
    Upsert(CompleteBlockSyncRequest),

    /// Received announcement about upcoming era validators.
    EraValidators {
        validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    },

    Next,

    DisconnectFromPeer(NodeId),

    #[from]
    BlockFetched(FetchResult<Block>),
    BlockStored {
        block_hash: BlockHash,
        success: bool,
    },
    #[from]
    DeployFetched(FetchResult<Deploy>),
    DeployStored {
        block_hash: BlockHash,
        deploy_hash: DeployHash,
        success: bool,
    },
    #[from]
    FinalitySignaturesFetched(FetchResult<BlockSignatures>),
    FinalitySignaturesStored {
        block_hash: BlockHash,
        success: bool,
    },
    TrieOrChunkFetched {
        id: TrieOrChunkId,
        fetch_result: FetchResult<TrieOrChunk>,
    },
    TrieOrChunkStored {
        block_hash: BlockHash,
        id: TrieOrChunkId,
        success: bool,
    },
    ExecutionResultsOrChunkFetched {},
    ExecutionResultsOrChunkStored {
        block_hash: BlockHash,
        // id: TrieOrChunkId,
        success: bool,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        todo!()
    }
}
