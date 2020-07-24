use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use super::{Item, Message};
use crate::{components::small_network::NodeId, utils::DisplayIter};

/// `Gossiper` events.
#[derive(Debug)]
pub enum Event<T: Item> {
    /// A new item has been received to be gossiped.
    ItemReceived { item: Box<T> },
    /// The network component gossiped to the included peers.
    GossipedTo {
        item_id: T::Id,
        peers: HashSet<NodeId>,
    },
    /// The timeout for waiting for a gossip response has elapsed and we should check the response
    /// arrived.
    CheckGossipTimeout { item_id: T::Id, peer: NodeId },
    /// The timeout for waiting for the full item has elapsed and we should check the response
    /// arrived.
    CheckGetFromPeerTimeout { item_id: T::Id, peer: NodeId },
    /// An incoming gossip network message.
    MessageReceived { sender: NodeId, message: Message<T> },
    /// The result of the gossiper sending an item to the component responsible for holding it.  If
    /// the result is `Ok`, the item's ID should be gossiped onwards.
    PutToHolderResult {
        item_id: T::Id,
        maybe_sender: Option<NodeId>,
        result: Result<(), String>,
    },
    /// The result of the gossiper getting an item from the component responsible for holding it.
    /// If the result is `Ok`, the item should be sent to the requesting peer.
    GetFromHolderResult {
        item_id: T::Id,
        requester: NodeId,
        result: Box<Result<T, String>>,
    },
}

impl<T: Item> Display for Event<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ItemReceived { item } => write!(formatter, "new item received: {}", item.id()),
            Event::GossipedTo { item_id, peers } => write!(
                formatter,
                "gossiped {} to {}",
                item_id,
                DisplayIter::new(peers)
            ),
            Event::CheckGossipTimeout { item_id, peer } => write!(
                formatter,
                "check gossip timeout for {} with {}",
                item_id, peer
            ),
            Event::CheckGetFromPeerTimeout { item_id, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                item_id, peer
            ),
            Event::MessageReceived { sender, message } => {
                write!(formatter, "{} received from {}", message, sender)
            }
            Event::PutToHolderResult {
                item_id, result, ..
            } => {
                if result.is_ok() {
                    write!(formatter, "put {} to holder component", item_id)
                } else {
                    write!(formatter, "failed to put {} to holder component", item_id)
                }
            }
            Event::GetFromHolderResult {
                item_id, result, ..
            } => {
                if result.is_ok() {
                    write!(formatter, "got {} from holder component", item_id)
                } else {
                    write!(formatter, "failed to get {} from holder component", item_id)
                }
            }
        }
    }
}
