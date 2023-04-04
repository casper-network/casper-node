use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;
use tracing::error;

use crate::{components::fetcher::FetchItem, types::NodeId};

#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize)]
pub(crate) enum Error<T: FetchItem> {
    #[error("item with id {id:?} absent on peer {peer:?}")]
    Absent { id: Box<T::Id>, peer: NodeId },

    #[error("peer {peer:?} rejected fetch request for item with id {id:?}")]
    Rejected { id: Box<T::Id>, peer: NodeId },

    #[error("timed out getting item with id {id:?} from peer {peer:?}")]
    TimedOut { id: Box<T::Id>, peer: NodeId },

    #[error("could not construct get request for item with id {id:?} for peer {peer:?}")]
    CouldNotConstructGetRequest { id: Box<T::Id>, peer: NodeId },

    #[error(
        "ongoing fetch for {id} from {peer} has different validation metadata ({current:?}) to \
        that given in new fetch attempt ({new:?})"
    )]
    ValidationMetadataMismatch {
        id: Box<T::Id>,
        peer: NodeId,
        current: Box<T::ValidationMetadata>,
        new: Box<T::ValidationMetadata>,
    },
}

impl<T: FetchItem> Error<T> {
    pub(crate) fn is_peer_fault(&self) -> bool {
        match self {
            // The peer claimed to have the item, so it should not be absent.
            Error::Absent { .. } | Error::Rejected { .. } | Error::TimedOut { .. } => true,
            Error::CouldNotConstructGetRequest { .. }
            | Error::ValidationMetadataMismatch { .. } => false,
        }
    }

    pub(crate) fn id(&self) -> &T::Id {
        match self {
            Error::Absent { id, .. } => id,
            Error::Rejected { id, .. } => id,
            Error::TimedOut { id, .. } => id,
            Error::CouldNotConstructGetRequest { id, .. } => id,
            Error::ValidationMetadataMismatch { id, .. } => id,
        }
    }

    pub(crate) fn peer(&self) -> &NodeId {
        match self {
            Error::Absent { peer, .. }
            | Error::Rejected { peer, .. }
            | Error::TimedOut { peer, .. }
            | Error::CouldNotConstructGetRequest { peer, .. }
            | Error::ValidationMetadataMismatch { peer, .. } => peer,
        }
    }
}

impl<T: FetchItem> DataSize for Error<T>
where
    T::Id: DataSize,
{
    const IS_DYNAMIC: bool = <T::Id as DataSize>::IS_DYNAMIC;

    const STATIC_HEAP_SIZE: usize = <T::Id as DataSize>::STATIC_HEAP_SIZE;

    fn estimate_heap_size(&self) -> usize {
        match self {
            Error::Absent { id, .. }
            | Error::Rejected { id, .. }
            | Error::TimedOut { id, .. }
            | Error::CouldNotConstructGetRequest { id, .. } => id.estimate_heap_size(),
            Error::ValidationMetadataMismatch {
                id, current, new, ..
            } => id.estimate_heap_size() + current.estimate_heap_size() + new.estimate_heap_size(),
        }
    }
}
