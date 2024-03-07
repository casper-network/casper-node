//! Announcement effects.
//!
//! Announcements indicate new incoming data or events from various sources. See the top-level
//! module documentation for details.

use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
    fs::File,
    sync::Arc,
};

use datasize::DataSize;
use itertools::Itertools;
use serde::Serialize;

use casper_types::{
    execution::Effects, Block, DeployHash, EraId, FinalitySignature, FinalitySignatureV2,
    NextUpgrade, PublicKey, Timestamp, Transaction, U512,
};

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        diagnostics_port::FileSerializer,
        fetcher::FetchItem,
        gossiper::GossipItem,
        network::blocklist::BlocklistJustification,
    },
    effect::Responder,
    failpoints::FailpointActivation,
    types::{FinalizedBlock, MetaBlock, NodeId},
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
    /// A shutdown has been requested by the user.
    ShutdownDueToUserRequest,

    /// The node should shut down with exit code 0 in readiness for the next binary to start.
    ShutdownForUpgrade,

    /// The component has encountered a fatal error and cannot continue.
    ///
    /// This usually triggers a shutdown of the application.
    FatalError {
        file: &'static str,
        line: u32,
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
    /// Activates/deactivates a failpoint.
    ActivateFailpoint {
        /// The failpoint activation to process.
        activation: FailpointActivation,
    },
}

impl Debug for ControlAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ControlAnnouncement::ShutdownDueToUserRequest => write!(f, "ShutdownDueToUserRequest"),
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
            ControlAnnouncement::ActivateFailpoint { activation } => f
                .debug_struct("ActivateFailpoint")
                .field("activation", activation)
                .finish(),
        }
    }
}

impl Display for ControlAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ControlAnnouncement::ShutdownDueToUserRequest => {
                write!(f, "shutdown due to user request")
            }
            ControlAnnouncement::ShutdownForUpgrade => write!(f, "shutdown for upgrade"),
            ControlAnnouncement::FatalError { file, line, msg } => {
                write!(f, "fatal error [{}:{}]: {}", file, line, msg)
            }
            ControlAnnouncement::QueueDumpRequest { .. } => {
                write!(f, "dump event queue")
            }
            ControlAnnouncement::ActivateFailpoint { activation } => {
                write!(f, "failpoint activation: {}", activation)
            }
        }
    }
}

/// A component has encountered a fatal error and cannot continue.
///
/// This usually triggers a shutdown of the application.
#[derive(Serialize, Debug)]
#[must_use]
pub(crate) struct FatalAnnouncement {
    pub(crate) file: &'static str,
    pub(crate) line: u32,
    pub(crate) msg: String,
}

impl Display for FatalAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "fatal error [{}:{}]: {}", self.file, self.line, self.msg)
    }
}

#[derive(DataSize, Serialize, Debug)]
pub(crate) struct MetaBlockAnnouncement(pub(crate) MetaBlock);

impl Display for MetaBlockAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "announcement for meta block {} at height {}",
            self.0.hash(),
            self.0.height(),
        )
    }
}

#[derive(DataSize, Serialize, Debug)]
pub(crate) struct UnexecutedBlockAnnouncement(pub(crate) u64);

impl Display for UnexecutedBlockAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "announcement for unexecuted finalized block at height {}",
            self.0,
        )
    }
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

/// A `TransactionAcceptor` announcement.
#[derive(Debug, Serialize)]
pub(crate) enum TransactionAcceptorAnnouncement {
    /// A transaction which wasn't previously stored on this node has been accepted and stored.
    AcceptedNewTransaction {
        /// The new transaction.
        transaction: Arc<Transaction>,
        /// The source (peer or client) of the transaction.
        source: Source,
    },

    /// An invalid transaction was received.
    InvalidTransaction {
        /// The invalid transaction.
        transaction: Transaction,
        /// The source (peer or client) of the transaction.
        source: Source,
    },
}

impl Display for TransactionAcceptorAnnouncement {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionAcceptorAnnouncement::AcceptedNewTransaction {
                transaction,
                source,
            } => write!(
                formatter,
                "accepted new transaction {} from {}",
                transaction.hash(),
                source
            ),
            TransactionAcceptorAnnouncement::InvalidTransaction {
                transaction,
                source,
            } => {
                write!(
                    formatter,
                    "invalid transaction {} from {}",
                    transaction.hash(),
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
                "Validator fault with public key: {} has been identified at time: {} in {}",
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
pub(crate) enum GossiperAnnouncement<T: GossipItem> {
    /// A new gossip has been received, but not necessarily the full item.
    GossipReceived { item_id: T::Id, sender: NodeId },

    /// A new item has been received, where the item's ID is the complete item.
    NewCompleteItem(T::Id),

    /// A new item has been received where the item's ID is NOT the complete item.
    NewItemBody { item: Box<T>, sender: NodeId },

    /// Finished gossiping about the indicated item.
    FinishedGossiping(T::Id),
}

impl<T: GossipItem> Display for GossiperAnnouncement<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GossiperAnnouncement::GossipReceived { item_id, sender } => {
                write!(f, "new gossiped item {} from sender {}", item_id, sender)
            }
            GossiperAnnouncement::NewCompleteItem(item) => write!(f, "new complete item {}", item),
            GossiperAnnouncement::NewItemBody { item, sender } => {
                write!(f, "new item body {} from {}", item.gossip_id(), sender)
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
    /// A step was committed successfully and has altered global state.
    CommitStepSuccess {
        /// The era id in which the step was committed to global state.
        era_id: EraId,
        /// The operations and transforms committed to global state.
        effects: Effects,
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
    /// A finality signature which wasn't previously stored on this node has been accepted and
    /// stored.
    AcceptedNewFinalitySignature {
        finality_signature: Box<FinalitySignatureV2>,
    },
}

impl Display for BlockAccumulatorAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockAccumulatorAnnouncement::AcceptedNewFinalitySignature { finality_signature } => {
                write!(
                    f,
                    "finality signature {} accepted",
                    finality_signature.gossip_id()
                )
            }
        }
    }
}

/// A block which wasn't previously stored on this node has been fetched and stored.
#[derive(Debug, Serialize)]
pub(crate) struct FetchedNewBlockAnnouncement {
    pub(crate) block: Arc<Block>,
    pub(crate) peer: NodeId,
}

impl Display for FetchedNewBlockAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "new block {} fetched from {}",
            self.block.fetch_id(),
            self.peer
        )
    }
}

/// A finality signature which wasn't previously stored on this node has been fetched and stored.
#[derive(Debug, Serialize)]
pub(crate) struct FetchedNewFinalitySignatureAnnouncement {
    pub(crate) finality_signature: Box<FinalitySignature>,
    pub(crate) peer: NodeId,
}

impl Display for FetchedNewFinalitySignatureAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "new finality signature {} fetched from {}",
            self.finality_signature.fetch_id(),
            self.peer
        )
    }
}
