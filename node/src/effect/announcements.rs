//! Announcement effects.
//!
//! Announcements indicate new incoming data or events from various sources. See the top-level
//! module documentation for details.

use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use casper_types::ExecutionResult;

use crate::{
    components::small_network::GossipedAddress,
    types::{
        Block, BlockHash, BlockHeader, Deploy, DeployHash, DeployHeader, FinalizedBlock, Item,
    },
    utils::Source,
};

/// A networking layer announcement.
#[derive(Debug)]
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
#[derive(Debug)]
#[must_use]
pub enum RpcServerAnnouncement {
    /// A new deploy received.
    DeployReceived {
        /// The received deploy.
        deploy: Box<Deploy>,
    },
}

impl Display for RpcServerAnnouncement {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RpcServerAnnouncement::DeployReceived { deploy } => {
                write!(formatter, "api server received {}", deploy.id())
            }
        }
    }
}

/// A `DeployAcceptor` announcement.
#[derive(Debug)]
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
pub enum ConsensusAnnouncement {
    /// A block was finalized.
    Finalized(Box<FinalizedBlock>),
    /// A linear chain block has been handled.
    Handled(Box<BlockHeader>),
}

impl Display for ConsensusAnnouncement {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConsensusAnnouncement::Finalized(block) => {
                write!(formatter, "finalized proto block {}", block)
            }
            ConsensusAnnouncement::Handled(block_header) => write!(
                formatter,
                "Linear chain block has been handled by consensus, height={}, hash={}",
                block_header.height(),
                block_header.hash()
            ),
        }
    }
}

/// A BlockExecutor announcement.
#[derive(Debug)]
pub enum BlockExecutorAnnouncement {
    /// A new block from the linear chain was produced.
    LinearChainBlock {
        /// The block.
        block: Block,
        /// The results of executing the deploys in this block.
        execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
    },
}

impl Display for BlockExecutorAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockExecutorAnnouncement::LinearChainBlock { block, .. } => {
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
        /// Block header.
        block_header: Box<BlockHeader>,
    },
}

impl Display for LinearChainAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            LinearChainAnnouncement::BlockAdded { block_hash, .. } => {
                write!(f, "block added {}", block_hash)
            }
        }
    }
}
