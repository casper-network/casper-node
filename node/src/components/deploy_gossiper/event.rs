use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use super::Message;
use crate::{
    components::{small_network::NodeId, storage},
    types::{Deploy, DeployHash},
    utils::DisplayIter,
};

/// `DeployGossiper` events.
#[derive(Debug)]
pub enum Event {
    /// A new deploy has been received to be gossiped.
    DeployReceived { deploy: Box<Deploy> },
    /// The network component gossiped to the included peers.
    GossipedTo {
        deploy_hash: DeployHash,
        peers: HashSet<NodeId>,
    },
    /// The timeout for waiting for a gossip response has elapsed and we should check the response
    /// arrived.
    CheckGossipTimeout {
        deploy_hash: DeployHash,
        peer: NodeId,
    },
    /// The timeout for waiting for the full deploy body has elapsed and we should check the
    /// response arrived.
    CheckGetFromPeerTimeout {
        deploy_hash: DeployHash,
        peer: NodeId,
    },
    /// An incoming gossip network message.
    MessageReceived { sender: NodeId, message: Message },
    /// The result of the `DeployGossiper` putting a deploy to the storage component.  If the
    /// result is `Ok`, the deploy hash should be gossiped onwards.
    PutToStoreResult {
        deploy_hash: DeployHash,
        maybe_sender: Option<NodeId>,
        result: storage::Result<bool>,
    },
    /// The result of the `DeployGossiper` getting a deploy from the storage component.  If the
    /// result is `Ok`, the deploy should be sent to the requesting peer.
    GetFromStoreResult {
        deploy_hash: DeployHash,
        requester: NodeId,
        result: Box<storage::Result<Deploy>>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::DeployReceived { deploy } => {
                write!(formatter, "new deploy received: {}", deploy.id())
            }
            Event::GossipedTo { deploy_hash, peers } => write!(
                formatter,
                "gossiped {} to {}",
                deploy_hash,
                DisplayIter::new(peers)
            ),
            Event::CheckGossipTimeout { deploy_hash, peer } => write!(
                formatter,
                "check gossip timeout for {} with {}",
                deploy_hash, peer
            ),
            Event::CheckGetFromPeerTimeout { deploy_hash, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                deploy_hash, peer
            ),
            Event::MessageReceived { sender, message } => {
                write!(formatter, "{} received from {}", message, sender)
            }
            Event::PutToStoreResult {
                deploy_hash,
                result,
                ..
            } => {
                if result.is_ok() {
                    write!(formatter, "put {} to store", deploy_hash)
                } else {
                    write!(formatter, "failed to put {} to store", deploy_hash)
                }
            }
            Event::GetFromStoreResult {
                deploy_hash,
                result,
                ..
            } => {
                if result.is_ok() {
                    write!(formatter, "got {} from store", deploy_hash)
                } else {
                    write!(formatter, "failed to get {} from store", deploy_hash)
                }
            }
        }
    }
}
