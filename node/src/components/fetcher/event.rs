use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use serde::Serialize;

use super::Item;
use crate::{
    effect::{announcements::DeployAcceptorAnnouncement, requests::FetcherRequest, Responder},
    types::{Deploy, NodeId},
    utils::Source,
};

#[derive(Clone, DataSize, Debug, PartialEq)]
pub(crate) enum FetchResult<T, I> {
    FromStorage(Box<T>),
    FromPeer(Box<T>, I),
}

pub(crate) type FetchResponder<T> = Responder<Option<FetchResult<T, NodeId>>>;

/// `Fetcher` events.
#[derive(Debug, Serialize)]
pub(crate) enum Event<T: Item> {
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
    /// A different component rejected an item.
    // TODO: If having this event is not desirable, the `DeployAcceptorAnnouncement` needs to be
    //       split in two instead.
    RejectedRemotely {
        item: Box<T>,
        source: Source<NodeId>,
    },
    /// An item was not available on the remote peer.
    AbsentRemotely { id: T::Id, peer: NodeId },
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

// A deploy fetcher knows how to update its state if deploys are coming in via the deploy acceptor.
impl From<DeployAcceptorAnnouncement<NodeId>> for Event<Deploy> {
    #[inline]
    fn from(announcement: DeployAcceptorAnnouncement<NodeId>) -> Self {
        match announcement {
            DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source } => {
                Event::GotRemotely {
                    item: deploy,
                    source,
                }
            }
            DeployAcceptorAnnouncement::InvalidDeploy { deploy, source } => {
                Event::RejectedRemotely {
                    item: deploy,
                    source,
                }
            }
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
            Event::RejectedRemotely { item, source } => write!(
                formatter,
                "other component rejected {} from {}",
                item.id(),
                source
            ),
            Event::TimeoutPeer { id, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                id, peer
            ),
            Event::AbsentRemotely { id, peer } => {
                write!(formatter, "Item {} was not available on {}", id, peer)
            }
        }
    }
}
