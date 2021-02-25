//! Announcement effects.
//!
//! Announcements indicate new incoming data or events from various sources. See the top-level
//! module documentation for details.

use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use serde::Serialize;

use casper_types::{ExecutionResult, PublicKey};

use crate::{
    components::{
        chainspec_loader::NextUpgrade, consensus::EraId, deploy_acceptor::Error,
        small_network::GossipedAddress,
    },
    effect::Responder,
    types::{
        Block, BlockHash, Deploy, DeployHash, DeployHeader, FinalitySignature, FinalizedBlock,
        Item, Timestamp,
    },
    utils::Source,
};

/// A networking layer announcement.
#[derive(Debug, Serialize)]
#[must_use]
pub enum NetworkAnnouncement<I, P> {
    /// A payload message has been received from a peer.
    MessageReceived {
        /// The sender of the message
        sender: I,
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
    NewPeer(I),
}

impl<I, P> Display for NetworkAnnouncement<I, P>
where
    I: Display,
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
pub enum RpcServerAnnouncement {
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
pub enum DeployAcceptorAnnouncement<I> {
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
pub enum ConsensusAnnouncement<I> {
    /// A block was finalized.
    Finalized(Box<FinalizedBlock>),
    /// A linear chain block has been handled.
    Handled(Box<Block>),
    /// An equivocation has been detected.
    Fault {
        /// The Id of the era in which the equivocation was detected
        era_id: EraId,
        /// The public key of the equivocator.
        public_key: Box<PublicKey>,
        /// The timestamp when the evidence of the equivocation was detected.
        timestamp: Timestamp,
    },
    /// We want to disconnect from a peer due to its transgressions.
    DisconnectFromPeer(I),
}

impl<I> Display for ConsensusAnnouncement<I>
where
    I: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConsensusAnnouncement::Finalized(block) => {
                write!(formatter, "finalized proto block {}", block)
            }
            ConsensusAnnouncement::Handled(block) => write!(
                formatter,
                "Linear chain block has been handled by consensus, height={}, hash={}",
                block.height(),
                block.hash()
            ),
            ConsensusAnnouncement::Fault {
                era_id,
                public_key,
                timestamp,
            } => write!(
                formatter,
                "Validator fault with public key: {} has been identified at time: {} in era: {}",
                public_key, timestamp, era_id,
            ),
            ConsensusAnnouncement::DisconnectFromPeer(peer) => {
                write!(formatter, "Consensus wanting to disconnect from {}", peer)
            }
        }
    }
}

/// A ContractRuntime announcement.
#[derive(Debug)]
pub enum ContractRuntimeAnnouncement {
    /// A new block from the linear chain was produced.
    LinearChainBlock {
        /// The block.
        block: Block,
        /// The results of executing the deploys in this block.
        execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
    },
}

impl Display for ContractRuntimeAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ContractRuntimeAnnouncement::LinearChainBlock { block, .. } => {
                write!(f, "created linear chain block {}", block.hash())
            }
        }
    }
}

/// A Gossiper announcement.
#[derive(Debug)]
pub enum GossiperAnnouncement<T: Item> {
    /// A new item has been received, where the item's ID is the complete item.
    NewCompleteItem(T::Id),
}

impl<T: Item> Display for GossiperAnnouncement<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GossiperAnnouncement::NewCompleteItem(item) => write!(f, "new complete item {}", item),
        }
    }
}

/// A linear chain announcement.
#[derive(Debug)]
pub enum LinearChainAnnouncement {
    /// A new block has been created and stored locally.
    BlockAdded {
        /// Block hash.
        block_hash: BlockHash,
        /// Block.
        block: Box<Block>,
    },
    /// New finality signature received.
    NewFinalitySignature(Box<FinalitySignature>),
}

impl Display for LinearChainAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            LinearChainAnnouncement::BlockAdded { block_hash, .. } => {
                write!(f, "block added {}", block_hash)
            }
            LinearChainAnnouncement::NewFinalitySignature(fs) => {
                write!(f, "new finality signature {}", fs.block_hash)
            }
        }
    }
}

/// A chainspec loader announcement.
#[derive(Debug, Serialize)]
pub enum ChainspecLoaderAnnouncement {
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
