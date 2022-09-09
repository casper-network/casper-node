use serde::Serialize;
use thiserror::Error;
use tracing::error;

use crate::types::{FetcherItem, NodeId};

#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize)]
pub(crate) enum Error<T: FetcherItem> {
    #[error("Could not fetch item with id {id:?} from peer {peer:?}")]
    Absent { id: T::Id, peer: NodeId },

    #[error("Peer {peer:?} rejected fetch request for item with id {id:?}")]
    Rejected { id: T::Id, peer: NodeId },

    #[error("Timed out getting item with id {id:?} from peer {peer:?}")]
    TimedOut { id: T::Id, peer: NodeId },

    #[error("Could not construct get request for item with id {id:?} for peer {peer:?}")]
    CouldNotConstructGetRequest { id: T::Id, peer: NodeId },
}

impl<T: FetcherItem> Error<T> {
    pub(crate) fn peer(&self) -> &NodeId {
        match self {
            Error::Absent { peer, .. }
            | Error::Rejected { peer, .. }
            | Error::TimedOut { peer, .. }
            | Error::CouldNotConstructGetRequest { peer, .. } => peer,
        }
    }
}
