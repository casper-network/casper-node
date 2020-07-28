use std::fmt::{self, Display, Formatter};

use super::{Item, Message};
use crate::{
    components::storage::Result,
    effect::{requests::FetcherRequest, Responder},
    small_network::NodeId,
};

#[derive(Clone, Debug, PartialEq)]
pub enum FetchResult<T: Item> {
    FromStore(T),
    FromPeer(T, NodeId),
}

pub(crate) type FetchResponder<T: Item> = Responder<Option<Box<FetchResult<T>>>>;

#[derive(Debug, PartialEq)]
pub enum RequestDirection {
    Inbound,
    Outbound,
}

/// `Fetcher` events.
#[derive(Debug)]
pub enum Event<T: Item> {
    /// The initiating event to fetch an item by its id.
    Fetch {
        id: T::Id,
        peer: NodeId,
        responder: FetchResponder<T>,
    },
    /// The result of the `Fetcher` getting a item from the storage component.  If the
    /// result is not `Ok`, the item should be requested from the peer.
    GetFromStoreResult {
        request_direction: RequestDirection,
        id: T::Id,
        peer: NodeId,
        result: Box<Result<T>>,
    },
    /// The timeout has elapsed and we should clean up state.
    TimeoutPeer {
        id: T::Id,
        peer: NodeId,
    },
    /// An incoming network message.
    MessageReceived { sender: NodeId, payload: Message<T> },
    /// The result of the `Fetcher` putting a deploy to the storage component.
    StoredFromPeerResult {
        item: Box<T>,
        peer: NodeId,
        result: Result<bool>,
    },
}

impl<T: Item> From<FetcherRequest<NodeId, T>> for Event<T> {
    fn from(request: FetcherRequest<NodeId, T>) -> Self {
        match request {
            FetcherRequest::Fetch {
                id,
                peer,
                responder,
            } => Event::Fetch {
                id,
                peer,
                responder,
            },
        }
    }
}

impl<T: Item> Display for Event<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::TimeoutPeer { id, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                id, peer
            ),
            Event::Fetch { id, .. } => {
                write!(formatter, "request to fetch item at hash {}", id)
            }
            Event::MessageReceived { sender, payload: message } => {
                write!(formatter, "{} received from {}", message, sender)
            }
            Event::StoredFromPeerResult { item, result, .. } => {
                if result.is_ok() {
                    write!(formatter, "put {} to store", *item.id())
                } else {
                    write!(formatter, "failed to put {} to store", *item.id())
                }
            }
            Event::GetFromStoreResult {
                id,
                result,
                ..
            } => {
                if result.is_ok() {
                    write!(formatter, "got {} from store", id)
                } else {
                    write!(formatter, "failed to fetch {} from store", id)
                }
            }
        }
    }
}
