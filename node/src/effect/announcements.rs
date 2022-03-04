//! Announcement effects.
//!
//! Announcements indicate new incoming data or events from various sources. See the top-level
//! module documentation for details.

use std::fmt::{self, Display, Formatter};

use itertools::Itertools;
use serde::Serialize;

use casper_types::{EraId, ExecutionResult, PublicKey};

use crate::{
    components::{
        chainspec_loader::NextUpgrade, deploy_acceptor::Error, small_network::GossipedAddress,
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

/// A networking layer announcement.
#[derive(Debug, Serialize)]
#[must_use]
pub(crate) enum NetworkAnnouncement<P> {
    /// A payload message has been received from a peer.
    MessageReceived {
        /// The sender of the message
        sender: NodeId,
        /// The message payload
        payload: P,
    },
    /// Our public listening address should be gossiped across the network.
    GossipOurAddress(GossipedAddress),
    /// A new peer connection was established.
    ///
    /// IMPORTANT NOTE: This announcement is a work-around for some short-term functionality. Do
    ///                 not rely on or use this for anything without asking anyone that has written
    ///                 this section of the code first!
    NewPeer(NodeId),
}

impl<P> Display for NetworkAnnouncement<P>
where
    P: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkAnnouncement::MessageReceived { sender, payload } => {
                write!(formatter, "received from {}: {}", sender, payload)
            }
            NetworkAnnouncement::GossipOurAddress(_) => write!(formatter, "gossip our address"),
            NetworkAnnouncement::NewPeer(id) => {
                write!(formatter, "new peer connection established to {}", id)
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

/// A ContractRuntimeAnnouncement's block.
#[derive(Debug)]
pub(crate) struct LinearChainBlock {
    /// The block.
    pub(crate) block: Block,
    /// The results of executing the deploys in this block.
    pub(crate) execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
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

/// An announcement indicating that the validator/non-validator status of the node changed.
#[derive(Debug, Serialize)]
pub(crate) struct ValidatorStatusChangedAnnouncement {
    /// Whether or not we are a validator now.
    pub(crate) new_status: bool,
}

impl Display for ValidatorStatusChangedAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "validator status changed to {}", self.new_status)
    }
}
