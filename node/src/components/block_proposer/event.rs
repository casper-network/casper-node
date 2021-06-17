use std::fmt::{self, Formatter};

use datasize::DataSize;
use derive_more::From;
use fmt::Display;
use serde::{Deserialize, Serialize};

use super::BlockHeight;
use crate::{
    effect::requests::BlockProposerRequest,
    types::{Deploy, DeployHash, DeployHeader, FinalizedBlock, LoadedObject},
};
use casper_execution_engine::shared::motes::Motes;

/// Information about a deploy.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct DeployInfo {
    pub header: DeployHeader,
    pub payment_amount: Motes,
    pub size: usize,
}

/// An event for when using the block proposer as a component.
#[derive(DataSize, Debug, From)]
pub enum Event {
    /// Incoming `BlockProposerRequest`.
    #[from]
    Request(BlockProposerRequest),
    /// The chainspec and previous sets have been successfully loaded from storage.
    Loaded {
        /// Previously finalized deploys.
        finalized_deploys: Vec<(DeployHash, DeployHeader)>,
        /// The height of the next expected finalized block.
        next_finalized_block: BlockHeight,
    },
    /// A new deploy has been received by this node and gossiped: it should be retrieved from
    /// storage and buffered here.
    BufferDeploy(DeployHash),
    /// The given deploy has been gossiped and can now be included in a block.
    GotFromStorage(LoadedObject<Deploy>),
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
            Event::BufferDeploy(hash) => write!(f, "block-proposer add {}", hash),
            Event::GotFromStorage(deploy) => {
                write!(f, "block-proposer got from storage {}", deploy.id())
            }
            Event::Prune => write!(f, "block-proposer prune"),
            Event::FinalizedBlock(block) => {
                write!(f, "block-proposer finalized block {}", block)
            }
        }
    }
}
