use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use derive_more::From;
use serde::Serialize;

use casper_types::DisplayIter;

use super::GossipItem;
use crate::{
    effect::{incoming::GossiperIncoming, requests::BeginGossipRequest, GossipTarget},
    types::NodeId,
    utils::Source,
};

/// `Gossiper` events.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event<T: GossipItem> {
    /// A request to gossip an item has been made.
    #[from]
    BeginGossipRequest(BeginGossipRequest<T>),
    /// A new item has been received to be gossiped.
    ItemReceived {
        item_id: T::Id,
        source: Source,
        target: GossipTarget,
    },
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
    /// The timeout for waiting for a different component to validate and store the item has
    /// elapsed and we should check that `ItemReceived` has been called by now.
    CheckItemReceivedTimeout { item_id: T::Id },
    /// The result of the gossiper checking if an item exists in storage.
    IsStoredResult {
        item_id: T::Id,
        sender: NodeId,
        result: bool,
    },
    /// The result of the gossiper getting an item from storage. If the result is `Some`, the item
    /// should be sent to the requesting peer.
    GetFromStorageResult {
        item_id: T::Id,
        requester: NodeId,
        maybe_item: Option<Box<T>>,
    },
}

impl<T: GossipItem> Display for Event<T> {
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
            Event::ItemReceived {
                item_id, source, ..
            } => {
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
            Event::CheckItemReceivedTimeout { item_id } => {
                write!(formatter, "check item received timeout for {}", item_id,)
            }
            Event::IsStoredResult {
                item_id,
                sender,
                result,
            } => {
                write!(
                    formatter,
                    "{} is stored for gossip message from {}: {}",
                    item_id, sender, result
                )
            }
            Event::GetFromStorageResult {
                item_id,
                maybe_item,
                ..
            } => {
                if maybe_item.is_some() {
                    write!(formatter, "got {} from storage", item_id)
                } else {
                    write!(formatter, "failed to get {} from storage", item_id)
                }
            }
        }
    }
}
