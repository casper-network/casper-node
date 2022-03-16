//! Announcement effects.
//!
//! Announcements indicate new incoming data or events from various sources. See the top-level
//! module documentation for details.

use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
    fs::File,
};

use itertools::Itertools;
use serde::Serialize;

use casper_types::{EraId, ExecutionEffect, ExecutionResult, PublicKey, U512};

use crate::{
    components::{
        chainspec_loader::NextUpgrade, deploy_acceptor::Error, diagnostics_port::FileSerializer,
    },
    effect::Responder,
    types::{
        Block, Deploy, DeployHash, DeployHeader, FinalitySignature, FinalizedBlock, Item, NodeId,
        Timestamp,
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
#[derive(Serialize)]
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
    // An external event queue dump has been requested.
    QueueDumpRequest {
        /// The format to dump the queue in.
        #[serde(skip)]
        dump_format: QueueDumpFormat,
        /// Responder called when the dump has been finished.
        finished: Responder<()>,
    },
}

/// Queue dump format with handler.
#[derive(Serialize)]
pub(crate) enum QueueDumpFormat {
    /// Dump using given serde serializer.
    Serde(#[serde(skip)] FileSerializer),
    /// Dump writing debug output to file.
    Debug(#[serde(skip)] File),
}

impl QueueDumpFormat {
    /// Creates a new queue dump serde format.
    pub(crate) fn serde(serializer: FileSerializer) -> Self {
        QueueDumpFormat::Serde(serializer)
    }

    /// Creates a new queue dump debug format.
    pub(crate) fn debug(file: File) -> Self {
        QueueDumpFormat::Debug(file)
    }
}

impl Debug for ControlAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FatalError { file, line, msg } => f
                .debug_struct("FatalError")
                .field("file", file)
                .field("line", line)
                .field("msg", msg)
                .finish(),
            Self::QueueDumpRequest { .. } => f.debug_struct("QueueDump").finish_non_exhaustive(),
        }
    }
}

impl Display for ControlAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ControlAnnouncement::FatalError { file, line, msg } => {
                write!(f, "fatal error [{}:{}]: {}", file, line, msg)
            }
            ControlAnnouncement::QueueDumpRequest { .. } => {
                write!(f, "dump event queue")
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
pub(crate) enum DeployAcceptorAnnouncement {
    /// A deploy which wasn't previously stored on this node has been accepted and stored.
    AcceptedNewDeploy {
        /// The new deploy.
        deploy: Box<Deploy>,
        /// The source (peer or client) of the deploy.
        source: Source,
    },

    /// An invalid deploy was received.
    InvalidDeploy {
        /// The invalid deploy.
        deploy: Box<Deploy>,
        /// The source (peer or client) of the deploy.
        source: Source,
    },
}

impl Display for DeployAcceptorAnnouncement {
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

// A block proposer announcement.
#[derive(Debug, Serialize)]
pub(crate) enum BlockProposerAnnouncement {
    /// Hashes of the deploys that expired.
    DeploysExpired(Vec<DeployHash>),
}

impl Display for BlockProposerAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockProposerAnnouncement::DeploysExpired(hashes) => {
                write!(f, "pruned hashes: {}", hashes.iter().join(", "))
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
pub(crate) enum BlocklistAnnouncement {
    /// A given peer committed a blockable offense.
    OffenseCommitted(Box<NodeId>),
}

impl Display for BlocklistAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlocklistAnnouncement::OffenseCommitted(peer) => {
                write!(f, "peer {} committed offense", peer)
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

/// A ContractRuntime announcement.
#[derive(Debug, Serialize)]
pub(crate) enum ContractRuntimeAnnouncement {
    /// A new block from the linear chain was produced.
    LinearChainBlock {
        /// The block.
        block: Box<Block>,
        /// The results of executing the deploys in this block.
        // #[serde(skip_serializing)]
        execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
    },
    /// A step was committed successfully and has altered global state.
    CommitStepSuccess {
        /// The era id in which the step was committed to global state.
        era_id: EraId,
        /// The operations and transforms committed to global state.
        execution_effect: ExecutionEffect,
    },
    /// New era validators.
    UpcomingEraValidators {
        /// The era id in which the step was committed to global state.
        era_that_is_ending: EraId,
        /// The validators for the eras after the `era_that_is_ending` era.
        upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    },
}

impl Display for ContractRuntimeAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeAnnouncement::LinearChainBlock { block, .. } => {
                write!(f, "created linear chain block {}", block.hash())
            }
            ContractRuntimeAnnouncement::CommitStepSuccess { era_id, .. } => {
                write!(f, "commit step completed for {}", era_id)
            }
            ContractRuntimeAnnouncement::UpcomingEraValidators {
                era_that_is_ending, ..
            } => {
                write!(
                    f,
                    "upcoming era validators after current {}.",
                    era_that_is_ending,
                )
            }
        }
    }
}
