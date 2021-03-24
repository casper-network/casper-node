use std::fmt::{self, Formatter};

use datasize::DataSize;
use derive_more::From;
use fmt::Display;
use serde::{Deserialize, Serialize};

use super::BlockHeight;
use crate::{
    effect::requests::BlockProposerRequest,
    types::{DeployHash, DeployHeader, ProtoBlock},
};
use casper_execution_engine::shared::motes::Motes;

/// A wrapper over `DeployHeader` to differentiate between wasm-less transfers and wasm headers.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
pub enum DeployType {
    /// Represents a wasm-less transfer.
    Transfer {
        header: DeployHeader,
        payment_amount: Motes,
        size: usize,
    },
    /// Represents a wasm deploy.
    Other {
        header: DeployHeader,
        payment_amount: Motes,
        size: usize,
    },
}

impl DeployType {
    /// Access header in all variants of `DeployType`.
    pub fn header(&self) -> &DeployHeader {
        match self {
            Self::Transfer { header, .. } => header,
            Self::Other { header, .. } => header,
        }
    }

    /// Extract into header and drop `DeployType`.
    pub fn take_header(self) -> DeployHeader {
        match self {
            Self::Transfer { header, .. } => header,
            Self::Other { header, .. } => header,
        }
    }

    /// Access payment_amount from all variants.
    pub fn payment_amount(&self) -> Motes {
        match self {
            Self::Transfer { payment_amount, .. } => *payment_amount,
            Self::Other { payment_amount, .. } => *payment_amount,
        }
    }

    /// Access size from all variants.
    pub fn size(&self) -> usize {
        match self {
            Self::Transfer { size, .. } => *size,
            Self::Other { size, .. } => *size,
        }
    }

    /// Asks if the variant is a Transfer.
    pub fn is_transfer(&self) -> bool {
        matches!(self, DeployType::Transfer { .. })
    }

    /// Asks if the variant is Wasm.
    pub fn is_wasm(&self) -> bool {
        matches!(self, DeployType::Other { .. })
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
        /// Previously finalized deploys.
        finalized_deploys: Vec<(DeployHash, DeployHeader)>,
        /// The height of the next expected finalized block.
        next_finalized_block: BlockHeight,
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
        block: ProtoBlock,
        height: BlockHeight,
    },
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
