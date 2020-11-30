use std::{
    fmt::{self, Formatter},
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;
use fmt::Display;

use super::{BlockHeight, BlockProposerDeploySets};
use crate::{
    effect::requests::BlockProposerRequest,
    types::{DeployHash, DeployHeader, ProtoBlock},
    Chainspec,
};

/// An event for when using the block proposer as a component.
#[derive(DataSize, Debug, From)]
pub enum Event {
    /// Incoming `BlockProposerRequest`.
    #[from]
    Request(BlockProposerRequest),
    /// The chainspec and previous sets have been successfully loaded from storage.
    Loaded {
        /// Loaded chainspec.
        chainspec: Arc<Chainspec>,
        /// Loaded previously stored block proposer sets.
        sets: Option<BlockProposerDeploySets>,
    },
    /// A new deploy should be buffered.
    Buffer {
        hash: DeployHash,
        header: Box<DeployHeader>,
    },
    /// The deploy-buffer has been asked to prune stale deploys
    BufferPrune,
    /// A proto block has been finalized. We should never propose its deploys again.
    FinalizedProtoBlock {
        block: ProtoBlock,
        height: BlockHeight,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(req) => write!(f, "block-proposer request: {}", req),
            Event::Loaded {
                sets: Some(sets), ..
            } => write!(f, "loaded block-proposer deploy sets: {}", sets),
            Event::Loaded { sets: None, .. } => write!(
                f,
                "loaded block-proposer deploy sets, none found in storage"
            ),
            Event::Buffer { hash, .. } => write!(f, "block-proposer add {}", hash),
            Event::BufferPrune => write!(f, "buffer prune"),
            Event::FinalizedProtoBlock { block, height } => {
                write!(
                    f,
                    "deploy-buffer finalized proto block {} at height {}",
                    block, height
                )
            }
        }
    }
}
