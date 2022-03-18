use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use derive_more::From;
use serde::Serialize;

use super::Item;
use crate::{
    effect::{incoming::GossiperIncoming, requests::BeginGossipRequest},
    types::NodeId,
    utils::{DisplayIter, Source},
};

/// `Gossiper` events.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event<T: Item> {
    /// A request to gossip an item has been made.
    #[from]
    BeginGossipRequest(BeginGossipRequest<T>),
    /// A new item has been received to be gossiped.
    ItemReceived { item_id: T::Id, source: Source },
    /// The network component gossiped to the included peers.
    GossipedTo {
        item_id: T::Id,
        requested_count: usize,
        peers: HashSet<NodeId>,
    },
    /// The timeout for waiting for a gossip response has elapsed and we should check the response
    /// arrived.
    CheckGossipTimeout { item_id: T::Id, peer: NodeId },
    /// The timeout for waiting for the full item has elapsed and we should check the response
    /// arrived.
    CheckGetFromPeerTimeout { item_id: T::Id, peer: NodeId },
    /// An incoming gossip network message.
    #[from]
    Incoming(GossiperIncoming<T>),
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
            Event::BeginGossipRequest(BeginGossipRequest {
                item_id, source, ..
            }) => {
                write!(
                    formatter,
                    "begin gossping new item {} received from {}",
                    item_id, source
                )
            }
            Event::ItemReceived { item_id, source } => {
                write!(formatter, "new item {} received from {}", item_id, source)
            }
            Event::GossipedTo { item_id, peers, .. } => write!(
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
            Event::Incoming(incoming) => {
                write!(formatter, "incoming: {}", incoming)
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
