use std::fmt::{self, Display, Formatter};

use super::Item;
use crate::{
    effect::{requests::FetcherRequest, Responder},
    small_network::NodeId,
    utils::Source,
};

#[derive(Clone, Debug, PartialEq)]
pub enum FetchResult<T: Item> {
    FromStorage(Box<T>),
    FromPeer(Box<T>, NodeId),
}

pub(crate) type FetchResponder<T> = Responder<Option<FetchResult<T>>>;

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
    /// result is `None`, the item should be requested from the peer.
    GetFromStorageResult {
        id: T::Id,
        peer: NodeId,
        maybe_item: Box<Option<T>>,
    },
    /// An announcement from a different component that we have accepted and stored the given item.
    GotRemotely {
        item: Box<T>,
        source: Source<NodeId>,
    },
    /// The timeout has elapsed and we should clean up state.
    TimeoutPeer { id: T::Id, peer: NodeId },
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
            Event::Fetch { id, .. } => write!(formatter, "request to fetch item at hash {}", id),
            Event::GetFromStorageResult { id, maybe_item, .. } => {
                if maybe_item.is_some() {
                    write!(formatter, "got {} from storage", id)
                } else {
                    write!(formatter, "failed to fetch {} from storage", id)
                }
            }
            Event::GotRemotely { item, source } => {
                write!(formatter, "got {} from {}", item.id(), source)
            }
            Event::TimeoutPeer { id, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                id, peer
            ),
        }
    }
}
