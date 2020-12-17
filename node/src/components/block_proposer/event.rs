use std::{
    fmt::{self, Formatter},
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;
use fmt::Display;
use serde::{Deserialize, Serialize};

use super::{BlockHeight, BlockProposerDeploySets};
use crate::{
    effect::requests::BlockProposerRequest,
    types::{DeployHash, DeployHeader, ProtoBlock},
    Chainspec,
};

/// A wrapper over `DeployHeader` to differentiate between wasm-less transfers and wasm headers.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
pub enum DeployType {
    /// Represents a wasm-less transfer.
    Transfer(DeployHeader),
    /// Represents a wasm deploy.
    Wasm(DeployHeader),
}

impl DeployType {
    /// Access header in all variants of `DeployType`.
    pub fn header(&self) -> &DeployHeader {
        match self {
            Self::Transfer(header) => header,
            Self::Wasm(header) => header,
        }
    }
}

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
        sets: Box<Option<BlockProposerDeploySets>>,
    },
    /// A new deploy should be buffered.
    BufferDeploy {
        hash: DeployHash,
        deploy_type: Box<DeployType>,
    },
    /// The block proposer has been asked to prune stale deploys
    Prune,
    /// A proto block has been finalized. We should never propose its deploys again.
    FinalizedProtoBlock {
        block: Box<ProtoBlock>,
        height: BlockHeight,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(req) => write!(f, "block-proposer request: {}", req),
            Event::Loaded { sets, .. } => match &**sets {
                Some(deploy_set) => write!(f, "loaded block-proposer deploy sets: {}", deploy_set),
                None => write!(
                    f,
                    "loaded block-proposer deploy sets, none found in storage"
                ),
            },
            Event::BufferDeploy { hash, .. } => write!(f, "block-proposer add {}", hash),
            Event::Prune => write!(f, "block-proposer prune"),
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
