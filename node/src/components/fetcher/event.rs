use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;
use tracing::error;

use super::FetcherItem;
use crate::{
    components::fetcher::FetchResponse,
    effect::{announcements::DeployAcceptorAnnouncement, requests::FetcherRequest, Responder},
    types::{Deploy, NodeId},
    utils::Source,
};

#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize)]
pub(crate) enum FetcherError<T>
where
    T: FetcherItem,
{
    #[error("Could not fetch item with id {id:?} from peer {peer:?}")]
    Absent { id: T::Id, peer: NodeId },

    #[error("Peer {peer:?} rejected fetch request for item with id {id:?}")]
    Rejected { id: T::Id, peer: NodeId },

    #[error("Timed out getting item with id {id:?} from peer {peer:?}")]
    TimedOut { id: T::Id, peer: NodeId },

    #[error("Could not construct get request for item with id {id:?} for peer {peer:?}")]
    CouldNotConstructGetRequest { id: T::Id, peer: NodeId },
}

#[derive(Clone, DataSize, Debug, PartialEq)]
pub(crate) enum FetchedData<T> {
    FromStorage { item: Box<T> },
    FromPeer { item: Box<T>, peer: NodeId },
}

impl<T> FetchedData<T> {
    pub(crate) fn from_storage(item: T) -> Self {
        FetchedData::FromStorage {
            item: Box::new(item),
        }
    }

    pub(crate) fn from_peer(item: T, peer: NodeId) -> Self {
        FetchedData::FromPeer {
            item: Box::new(item),
            peer,
        }
    }
}

pub(crate) type FetchResult<T> = Result<FetchedData<T>, FetcherError<T>>;

pub(crate) type FetchResponder<T> = Responder<FetchResult<T>>;

/// `Fetcher` events.
#[derive(Debug, Serialize)]
pub(crate) enum Event<T: FetcherItem> {
    /// The initiating event to fetch an item by its id.
    Fetch(FetcherRequest<T>),
    /// The result of the `Fetcher` getting a item from the storage component.  If the
    /// result is `None`, the item should be requested from the peer.
    GetFromStorageResult {
        id: T::Id,
        peer: NodeId,
        maybe_item: Box<Option<T>>,
        responder: FetchResponder<T>,
    },
    /// An announcement from a different component that we have accepted and stored the given item.
    GotRemotely { item: Box<T>, source: Source },
    /// A different component rejected an item.
    // TODO: If having this event is not desirable, the `DeployAcceptorAnnouncement` needs to be
    //       split in two instead.
    GotInvalidRemotely { id: T::Id, source: Source },
    /// An item was not available on the remote peer.
    AbsentRemotely { id: T::Id, peer: NodeId },
    /// An item was available on the remote peer, but it chose to not provide it.
    RejectedRemotely { id: T::Id, peer: NodeId },
    /// The timeout has elapsed and we should clean up state.
    TimeoutPeer { id: T::Id, peer: NodeId },
}

impl<T: FetcherItem> Event<T> {
    pub(crate) fn from_get_response_serialized_item(
        peer: NodeId,
        serialized_item: &[u8],
    ) -> Option<Self> {
        match bincode::deserialize::<FetchResponse<T, T::Id>>(serialized_item) {
            Ok(FetchResponse::Fetched(item)) => Some(Event::GotRemotely {
                item: Box::new(item),
                source: Source::Peer(peer),
            }),
            Ok(FetchResponse::NotFound(id)) => Some(Event::AbsentRemotely { id, peer }),
            Ok(FetchResponse::NotProvided(id)) => Some(Event::RejectedRemotely { id, peer }),
            Err(error) => {
                error!("failed to decode {:?} from {}: {:?}", T::TAG, peer, error);
                None
            }
        }
    }
}

impl<T: FetcherItem> From<FetcherRequest<T>> for Event<T> {
    fn from(fetcher_request: FetcherRequest<T>) -> Self {
        Event::Fetch(fetcher_request)
    }
}

// A deploy fetcher knows how to update its state if deploys are coming in via the deploy acceptor.
impl From<DeployAcceptorAnnouncement> for Event<Deploy> {
    #[inline]
    fn from(announcement: DeployAcceptorAnnouncement) -> Self {
        match announcement {
            DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source } => {
                Event::GotRemotely {
                    item: deploy,
                    source,
                }
            }
            DeployAcceptorAnnouncement::InvalidDeploy { deploy, source } => {
                Event::GotInvalidRemotely {
                    id: *Deploy::id(&deploy),
                    source,
                }
            }
        }
    }
}

impl<T: FetcherItem> Display for Event<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Fetch(FetcherRequest { id, .. }) => {
                write!(formatter, "request to fetch item at hash {}", id)
            }
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
            Event::GotInvalidRemotely { id, source } => {
                write!(formatter, "invalid item {} from {}", id, source)
            }
            Event::TimeoutPeer { id, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                id, peer
            ),
            Event::AbsentRemotely { id, peer } => {
                write!(formatter, "item {} was not available on {}", id, peer)
            }
            Event::RejectedRemotely { id, peer } => {
                write!(
                    formatter,
                    "request to fetch item {} was rejected by {}",
                    id, peer
                )
            }
        }
    }
}
