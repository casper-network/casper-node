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
use casper_execution_engine::shared::motes::Motes;

/// A wrapper over `DeployHeader` to differentiate between wasm-less transfers and wasm headers.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
pub enum DeployType {
    /// Represents a wasm-less transfer.
    Transfer {
        header: DeployHeader,
        payment_amount: Motes,
    },
    /// Represents a wasm deploy.
    Wasm(DeployHeader),
}

impl DeployType {
    /// Access header in all variants of `DeployType`.
    pub fn header(&self) -> &DeployHeader {
        match self {
            Self::Transfer { header, .. } => header,
            Self::Wasm(header) => header,
        }
    }

    /// If this DeployType is a Transfer, return the amount, otherwise None.
    pub fn payment_amount(&self) -> Option<Motes> {
        match self {
            Self::Transfer { payment_amount, .. } => Some(*payment_amount),
            Self::Wasm(_) => None,
        }
    }

    /// Asks if the variant is a Transfer.
    pub fn is_transfer(&self) -> bool {
        matches!(self, Self::Transfer{..})
    }

    /// Asks if the variant is Wasm.
    pub fn is_wasm(&self) -> bool {
        matches!(self, Self::Wasm(_))
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
        sets: Option<BlockProposerDeploySets>,
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
                sets: Some(sets), ..
            } => write!(f, "loaded block-proposer deploy sets: {}", sets),
            Event::Loaded { sets: None, .. } => write!(
                f,
                "loaded block-proposer deploy sets, none found in storage"
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
