//! Announcement effects.
//!
//! Announcements indicate new incoming data or events from various sources. See the top-level
//! module documentation for details.

use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use serde::Serialize;

use casper_types::{EraId, ExecutionEffect, ExecutionResult, PublicKey};

use crate::{
    components::{chainspec_loader::NextUpgrade, deploy_acceptor::Error},
    effect::Responder,
    types::{
        Block, Deploy, DeployHash, DeployHeader, FinalitySignature, FinalizedBlock, Item, Timestamp,
    },
    utils::Source,
};

/// Control announcements are special announcements handled directly by the runtime/runner.
///
/// Reactors are never passed control announcements back in and every reactor event must be able to
/// be constructed from a `ControlAnnouncement` to be run.
///
/// Control announcements also use a priority queue to ensure that a component that reports a fatal
/// error is given as few follow-up events as possible. However, there currently is no guarantee
/// that this happens.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) enum ControlAnnouncement {
    /// The component has encountered a fatal error and cannot continue.
    ///
    /// This usually triggers a shutdown of the component, reactor or whole application.
    FatalError {
        /// File the fatal error occurred in.
        file: &'static str,
        /// Line number where the fatal error occurred.
        line: u32,
        /// Error message.
        msg: String,
    },
}

impl Display for ControlAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ControlAnnouncement::FatalError { file, line, msg } => {
                write!(f, "fatal error [{}:{}]: {}", file, line, msg)
            }
        }
    }
}

/// An RPC API server announcement.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) enum RpcServerAnnouncement {
    /// A new deploy received.
    DeployReceived {
        /// The received deploy.
        deploy: Box<Deploy>,
        /// A client responder in the case where a client submits a deploy.
        responder: Option<Responder<Result<(), Error>>>,
    },
}

impl Display for RpcServerAnnouncement {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RpcServerAnnouncement::DeployReceived { deploy, .. } => {
                write!(formatter, "api server received {}", deploy.id())
            }
        }
    }
}

/// A `DeployAcceptor` announcement.
#[derive(Debug, Serialize)]
pub(crate) enum DeployAcceptorAnnouncement<I> {
    /// A deploy which wasn't previously stored on this node has been accepted and stored.
    AcceptedNewDeploy {
        /// The new deploy.
        deploy: Box<Deploy>,
        /// The source (peer or client) of the deploy.
        source: Source<I>,
    },

    /// An invalid deploy was received.
    InvalidDeploy {
        /// The invalid deploy.
        deploy: Box<Deploy>,
        /// The source (peer or client) of the deploy.
        source: Source<I>,
    },
}

impl<I: Display> Display for DeployAcceptorAnnouncement<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source } => write!(
                formatter,
                "accepted new deploy {} from {}",
                deploy.id(),
                source
            ),
            DeployAcceptorAnnouncement::InvalidDeploy { deploy, source } => {
                write!(formatter, "invalid deploy {} from {}", deploy.id(), source)
            }
        }
    }
}

/// A consensus announcement.
#[derive(Debug)]
pub(crate) enum ConsensusAnnouncement {
    /// A block was finalized.
    Finalized(Box<FinalizedBlock>),
    /// A finality signature was created.
    CreatedFinalitySignature(Box<FinalitySignature>),
    /// An equivocation has been detected.
    Fault {
        /// The Id of the era in which the equivocation was detected
        era_id: EraId,
        /// The public key of the equivocator.
        public_key: Box<PublicKey>,
        /// The timestamp when the evidence of the equivocation was detected.
        timestamp: Timestamp,
    },
}

impl Display for ConsensusAnnouncement {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConsensusAnnouncement::Finalized(block) => {
                write!(formatter, "finalized block payload {}", block)
            }
            ConsensusAnnouncement::CreatedFinalitySignature(fs) => {
                write!(formatter, "signed an executed block: {}", fs)
            }
            ConsensusAnnouncement::Fault {
                era_id,
                public_key,
                timestamp,
            } => write!(
                formatter,
                "Validator fault with public key: {} has been identified at time: {} in era: {}",
                public_key, timestamp, era_id,
            ),
        }
    }
}

/// A block-list related announcement.
#[derive(Debug, Serialize)]
pub(crate) enum BlocklistAnnouncement<I> {
    /// A given peer committed a blockable offense.
    OffenseCommitted(Box<I>),
}

impl<I> Display for BlocklistAnnouncement<I>
where
    I: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlocklistAnnouncement::OffenseCommitted(peer) => {
                write!(f, "peer {} committed offense", peer)
            }
        }
    }
}

/// A ContractRuntime announcement.
#[derive(Debug)]
pub(crate) enum ContractRuntimeAnnouncement {
    /// A new block from the linear chain was produced.
    LinearChainBlock(Box<LinearChainBlock>),
    /// A Step succeeded and has altered global state.
    StepSuccess {
        /// The era id in which the step was committed to global state.
        era_id: EraId,
        /// The operations and transforms committed to global state.
        execution_effect: ExecutionEffect,
    },
}

impl ContractRuntimeAnnouncement {
    /// Create a ContractRuntimeAnnouncement::LinearChainBlock from it's parts.
    pub(crate) fn linear_chain_block(
        block: Block,
        execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
    ) -> Self {
        Self::LinearChainBlock(Box::new(LinearChainBlock {
            block,
            execution_results,
        }))
    }

    /// Create a ContractRuntimeAnnouncement::StepSuccess from an execution effect.
    pub(crate) fn step_success(era_id: EraId, execution_effect: ExecutionEffect) -> Self {
        Self::StepSuccess {
            era_id,
            execution_effect,
        }
    }
}

/// A ContractRuntimeAnnouncement's block.
#[derive(Debug)]
pub(crate) struct LinearChainBlock {
    /// The block.
    pub(crate) block: Block,
    /// The results of executing the deploys in this block.
    pub(crate) execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
}

impl Display for ContractRuntimeAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeAnnouncement::LinearChainBlock(linear_chain_block) => {
                write!(
                    f,
                    "created linear chain block {}",
                    linear_chain_block.block.hash()
                )
            }
            ContractRuntimeAnnouncement::StepSuccess { era_id, .. } => {
                write!(f, "step completed for {}", era_id)
            }
        }
    }
}

/// A Gossiper announcement.
#[derive(Debug)]
pub(crate) enum GossiperAnnouncement<T: Item> {
    /// A new item has been received, where the item's ID is the complete item.
    NewCompleteItem(T::Id),

    /// Finished gossiping about the indicated item.
    FinishedGossiping(T::Id),
}

impl<T: Item> Display for GossiperAnnouncement<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GossiperAnnouncement::NewCompleteItem(item) => write!(f, "new complete item {}", item),
            GossiperAnnouncement::FinishedGossiping(item_id) => {
                write!(f, "finished gossiping {}", item_id)
            }
        }
    }
}

/// A linear chain announcement.
#[derive(Debug)]
pub(crate) enum LinearChainAnnouncement {
    /// A new block has been created and stored locally.
    BlockAdded(Box<Block>),
    /// New finality signature received.
    NewFinalitySignature(Box<FinalitySignature>),
}

impl Display for LinearChainAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            LinearChainAnnouncement::BlockAdded(block) => {
                write!(f, "block added {}", block.hash())
            }
            LinearChainAnnouncement::NewFinalitySignature(fs) => {
                write!(f, "new finality signature {}", fs.block_hash)
            }
        }
    }
}

/// A chainspec loader announcement.
#[derive(Debug, Serialize)]
pub(crate) enum ChainspecLoaderAnnouncement {
    /// New upgrade recognized.
    UpgradeActivationPointRead(NextUpgrade),
}

impl Display for ChainspecLoaderAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ChainspecLoaderAnnouncement::UpgradeActivationPointRead(next_upgrade) => {
                write!(f, "read {}", next_upgrade)
            }
        }
    }
}
