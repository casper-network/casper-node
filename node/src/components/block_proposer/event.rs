use std::{
    collections::BTreeSet,
    fmt::{self, Formatter},
};

use datasize::DataSize;
use derive_more::From;
use fmt::Display;
use serde::{Deserialize, Serialize};

use casper_types::Motes;

use super::{BlockHeight, CachedState};
use crate::{
    effect::requests::BlockProposerRequest,
    types::{Approval, Block, DeployHeader, DeployOrTransferHash, FinalizedBlock},
};

/// Information about a deploy.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct DeployInfo {
    pub header: DeployHeader,
    pub payment_amount: Motes,
    pub size: usize,
}

/// An event for when using the block proposer as a component.
#[derive(DataSize, Debug, From)]
pub(crate) enum Event {
    /// Get effects to initialize component after construction and before normal usage.
    Initialize,
    /// Incoming `BlockProposerRequest`.
    #[from]
    Request(BlockProposerRequest),
    /// The chainspec and previous sets have been successfully loaded from storage.
    Loaded {
        /// Previously finalized blocks.
        finalized_blocks: Vec<Block>,
        /// The height of the next expected finalized block.
        next_finalized_block_height: BlockHeight,
        /// The cached state retrieved from storage.
        cached_state: CachedState,
    },
    /// A new deploy has been received by this node and stored: it should be retrieved from storage
    /// and buffered here.
    BufferDeploy {
        hash: DeployOrTransferHash,
        approvals: BTreeSet<Approval>,
        deploy_info: Box<DeployInfo>,
    },
    /// The block proposer has been asked to prune stale deploys.
    Prune,
    /// A block has been finalized. We should never propose its deploys again.
    FinalizedBlock(Box<FinalizedBlock>),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Initialize => write!(f, "initialize"),
            Event::Request(req) => write!(f, "block-proposer request: {}", req),
            Event::Loaded {
                next_finalized_block_height,
                ..
            } => write!(
                f,
                "loaded block-proposer finalized deploys; expected next finalized block: {}",
                next_finalized_block_height
            ),
            Event::BufferDeploy { hash, .. } => write!(f, "block-proposer add {}", hash),
            Event::Prune => write!(f, "block-proposer prune"),
            Event::FinalizedBlock(block) => {
                write!(f, "block-proposer finalized block {}", block)
            }
        }
    }
}
