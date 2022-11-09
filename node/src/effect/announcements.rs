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

use casper_types::{EraId, ExecutionEffect, ExecutionResult, PublicKey, Timestamp, U512};

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        deploy_acceptor::Error,
        diagnostics_port::FileSerializer,
        small_network::blocklist::BlocklistJustification,
        upgrade_watcher::NextUpgrade,
    },
    effect::Responder,
    types::{
        ApprovalsHashes, Block, BlockHash, Deploy, DeployHash, DeployHeader, FinalitySignature,
        FinalitySignatureId, FinalizedBlock, GossiperItem, Item, NodeId,
    },
    utils::Source,
};

// #[derive(Serialize)]
// pub(crate) enum ControlLogicAnnouncement {
//     Initialize => {
// //          ctor puts into status and we handle pushing various init events here
// //         self.storage.is_init() -> state / or effects if it requires work to be done
//     },
//     GetCaughtUp => {
//         // <- keep on catching up
//         // > sync leap then have convo w accumul
//         // OR
//         // <- transition to keeping up
//         // <- fatal
//     }
//     StayCaughtUp => {
//         // <- keep on keeping up
//         // <- maybe enuff cycles to attempt 1 sync block
//         // <- get caught up
//         // <- fatal
//     }
// }

// impl Display for ControlLogicAnnouncement {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         match self {
//             ControlLogicAnnouncement::MissingValidatorSet { era_id } => {
//                 write!(f, "missing validator set for era {}", era_id)
//             }
//         }
//     }
// }
//
// impl Debug for ControlLogicAnnouncement {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         match self {
//             Self::MissingValidatorSet { era_id } => f
//                 .debug_struct("MissingValidatorSet")
//                 .field("era_id", era_id)
//                 .finish(),
//         }
//     }
// }

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
    /// The node should shut down with exit code 0 in readiness for the next binary to start.
    ShutdownForUpgrade,

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
    /// An external event queue dump has been requested.
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
            ControlAnnouncement::ShutdownForUpgrade => write!(f, "ShutdownForUpgrade"),
            ControlAnnouncement::FatalError { file, line, msg } => f
                .debug_struct("FatalError")
                .field("file", file)
                .field("line", line)
                .field("msg", msg)
                .finish(),
            ControlAnnouncement::QueueDumpRequest { .. } => {
                f.debug_struct("QueueDump").finish_non_exhaustive()
            }
        }
    }
}

impl Display for ControlAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ControlAnnouncement::ShutdownForUpgrade => write!(f, "shutdown for upgrade"),
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
                write!(formatter, "api server received {}", deploy.hash())
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
                deploy.hash(),
                source
            ),
            DeployAcceptorAnnouncement::InvalidDeploy { deploy, source } => {
                write!(
                    formatter,
                    "invalid deploy {} from {}",
                    deploy.hash(),
                    source
                )
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) enum DeployBufferAnnouncement {
    /// Hashes of the deploys that expired.
    DeploysExpired(Vec<DeployHash>),
}

impl Display for DeployBufferAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DeployBufferAnnouncement::DeploysExpired(hashes) => {
                write!(f, "pruned hashes: {}", hashes.iter().join(", "))
            }
        }
    }
}

/// A consensus announcement.
#[derive(Debug)]
pub(crate) enum ConsensusAnnouncement {
    /// A block was proposed.
    Proposed(Box<ProposedBlock<ClContext>>),
    /// A block was finalized.
    Finalized(Box<FinalizedBlock>),
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
            ConsensusAnnouncement::Proposed(block) => {
                write!(formatter, "proposed block payload {}", block)
            }
            ConsensusAnnouncement::Finalized(block) => {
                write!(formatter, "finalized block payload {}", block)
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

/// Notable / unexpected peer behavior has been detected by some part of the system.
#[derive(Debug, Serialize)]
pub(crate) enum PeerBehaviorAnnouncement {
    /// A given peer committed a blockable offense.
    OffenseCommitted {
        /// The peer ID of the offending node.
        offender: Box<NodeId>,
        /// Justification for blocking the peer.
        justification: Box<BlocklistJustification>,
    },
}

impl Display for PeerBehaviorAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PeerBehaviorAnnouncement::OffenseCommitted {
                offender,
                justification,
            } => {
                write!(f, "peer {} committed offense: {}", offender, justification)
            }
        }
    }
}

/// A Gossiper announcement.
#[derive(Debug)]
pub(crate) enum GossiperAnnouncement<T: GossiperItem> {
    /// A new gossip has been received, but not necessarily the full item.
    GossipReceived { item_id: T::Id, sender: NodeId },

    /// A new item has been received, where the item's ID is the complete item.
    NewCompleteItem(T::Id),

    /// A new item has been received where the item's ID is NOT the complete item.
    NewItemBody { item: Box<T>, sender: NodeId },

    /// Finished gossiping about the indicated item.
    FinishedGossiping(T::Id),
}

impl<T: GossiperItem> Display for GossiperAnnouncement<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GossiperAnnouncement::GossipReceived { item_id, sender } => {
                write!(f, "new gossiped item {} from sender {}", item_id, sender)
            }
            GossiperAnnouncement::NewCompleteItem(item) => write!(f, "new complete item {}", item),
            GossiperAnnouncement::NewItemBody { item, sender } => {
                write!(f, "new item body {} from {}", item.id(), sender)
            }
            GossiperAnnouncement::FinishedGossiping(item_id) => {
                write!(f, "finished gossiping {}", item_id)
            }
        }
    }
}

/// A chainspec loader announcement.
#[derive(Debug, Serialize)]
pub(crate) enum UpgradeWatcherAnnouncement {
    /// New upgrade recognized.
    UpgradeActivationPointRead(NextUpgrade),
}

impl Display for UpgradeWatcherAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            UpgradeWatcherAnnouncement::UpgradeActivationPointRead(next_upgrade) => {
                write!(f, "read {}", next_upgrade)
            }
        }
    }
}

/// A ContractRuntime announcement.
#[derive(Debug, Serialize)]
pub(crate) enum ContractRuntimeAnnouncement {
    /// A new block was executed.
    ExecutedBlock {
        /// The block.
        block: Box<Block>,
        /// Approval hashes.
        approvals_hashes: Box<ApprovalsHashes>,
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
            ContractRuntimeAnnouncement::ExecutedBlock { block, .. } => {
                write!(f, "executed block {}", block.hash())
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

#[derive(Debug, Serialize)]
pub(crate) enum BlockAccumulatorAnnouncement {
    /// A block which wasn't previously stored on this node has been accepted and stored.
    AcceptedNewBlock { block: Box<Block> },
    /// A finality signature which wasn't previously stored on this node has been accepted and
    /// stored.
    AcceptedNewFinalitySignature {
        finality_signature: Box<FinalitySignature>,
    },
}

impl Display for BlockAccumulatorAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockAccumulatorAnnouncement::AcceptedNewBlock { block } => {
                write!(f, "block {} accepted", block.hash())
            }
            BlockAccumulatorAnnouncement::AcceptedNewFinalitySignature { finality_signature } => {
                write!(f, "finality signature {} accepted", finality_signature.id())
            }
        }
    }
}
