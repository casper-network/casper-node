use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;
use tracing::error;

use casper_types::EraId;

use super::Item;
use crate::{
    components::fetcher::FetchedOrNotFound,
    effect::{announcements::DeployAcceptorAnnouncement, requests::FetcherRequest, Responder},
    types::{Deploy, NodeId},
    utils::Source,
};

#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize)]
pub(crate) enum FetcherError<T>
where
    T: Item,
{
    #[error("Could not fetch item with id {id:?} from peer {peer:?}")]
    Absent { id: T::Id, peer: NodeId },

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

pub(crate) type FetchResult<T> = Result<FetchedData<T>, FetcherError<T>>;

pub(crate) type FetchResponder<T> = Responder<FetchResult<T>>;

/// `Fetcher` events.
#[derive(Debug, Serialize)]
pub(crate) enum Event<T: Item> {
    /// The initiating event to fetch an item by its id.
    Fetch(FetcherRequest<T>),
    /// The result of the `Fetcher` getting a item from the storage component.  If the
    /// result is `None`, the item should be requested from the peer.
    GetFromStorageResult {
        id: T::Id,
        peer: NodeId,
        maybe_item: Box<Option<T>>,
    },
    /// An announcement from a different component that we have accepted and stored the given item.
    GotRemotely {
        // TODO[RC]: `verifiable_chunked_hash_activation` is scattered around, because this is a
        // piece of information obtained in the top-level code (read from the chainspec),
        // but required across all the layers, including the very bottom ones. At some
        // point we should consider refactoring to get rid of such "tramp data".
        verifiable_chunked_hash_activation: Option<EraId>,
        item: Box<T>,
        source: Source,
    },
    /// A different component rejected an item.
    // TODO: If having this event is not desirable, the `DeployAcceptorAnnouncement` needs to be
    //       split in two instead.
    RejectedRemotely { id: T::Id, source: Source },
    /// An item was not available on the remote peer.
    AbsentRemotely { id: T::Id, peer: NodeId },
    /// The timeout has elapsed and we should clean up state.
    TimeoutPeer { id: T::Id, peer: NodeId },
}

impl<T: Item> Event<T> {
    pub(crate) fn from_get_response_serialized_item(
        peer: NodeId,
        serialized_item: &[u8],
        verifiable_chunked_hash_activation: EraId,
    ) -> Option<Self> {
        match bincode::deserialize::<FetchedOrNotFound<T, T::Id>>(serialized_item) {
            Ok(FetchedOrNotFound::Fetched(item)) => Some(Event::GotRemotely {
                verifiable_chunked_hash_activation: Some(verifiable_chunked_hash_activation),
                item: Box::new(item),
                source: Source::Peer(peer),
            }),
            Ok(FetchedOrNotFound::NotFound(id)) => Some(Event::AbsentRemotely { id, peer }),
            Err(error) => {
                error!("failed to decode {:?} from {}: {:?}", T::TAG, peer, error);
                None
            }
        }
    }
}

impl<T: Item> From<FetcherRequest<T>> for Event<T> {
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
                    verifiable_chunked_hash_activation: None,
                    item: deploy,
                    source,
                }
            }
            DeployAcceptorAnnouncement::InvalidDeploy { deploy, source } => {
                Event::RejectedRemotely {
                    id: *Deploy::id(&deploy),
                    source,
                }
            }
        }
    }
}

impl<T: Item> Display for Event<T> {
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
            Event::GotRemotely {
                verifiable_chunked_hash_activation,
                item,
                source,
            } => {
                // If `verifiable_chunked_hash_activation` is not present here then our T is an
                // object that doesn't require the activation point to calculate its
                // `id`, hence we can provide any value to the `id()` method, as
                // it'll be ignored.
                let verifiable_chunked_hash_activation =
                    verifiable_chunked_hash_activation.unwrap_or_default();

                write!(
                    formatter,
                    "got {} from {}",
                    item.id(verifiable_chunked_hash_activation),
                    source
                )
            }
            Event::RejectedRemotely { id, source } => {
                write!(formatter, "other component rejected {} from {}", id, source)
            }
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
