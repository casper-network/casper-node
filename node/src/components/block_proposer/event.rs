use std::fmt::{self, Formatter};

use datasize::DataSize;
use derive_more::From;
use fmt::Display;
use serde::{Deserialize, Serialize};

use casper_types::Motes;

use super::BlockHeight;
use crate::{
    effect::requests::BlockProposerRequest,
    types::{DeployHash, DeployHeader, DeployOrTransferHash, FinalizedBlock, Timestamp},
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
    /// Incoming `BlockProposerRequest`.
    #[from]
    Request(BlockProposerRequest),
    /// The chainspec and previous sets have been successfully loaded from storage.
    Loaded {
        /// Previously finalized deploys.
        finalized_deploys: Vec<(DeployHash, DeployHeader)>,
        /// The height of the next expected finalized block.
        next_finalized_block: BlockHeight,
        last_finalized_timestamp: Timestamp,
    },
    /// A new deploy has been received by this node and stored: it should be retrieved from storage
    /// and buffered here.
    BufferDeploy {
        hash: DeployOrTransferHash,
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
            Event::Request(req) => write!(f, "block-proposer request: {}", req),
            Event::Loaded {
                next_finalized_block,
                ..
            } => write!(
                f,
                "loaded block-proposer finalized deploys; expected next finalized block: {}",
                next_finalized_block
            ),
            Event::BufferDeploy { hash, .. } => write!(f, "block-proposer add {}", hash),
            Event::Prune => write!(f, "block-proposer prune"),
            Event::FinalizedBlock(block) => {
                write!(f, "block-proposer finalized block {}", block)
            }
        }
    }
}
