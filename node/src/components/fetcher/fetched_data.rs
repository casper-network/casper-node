use datasize::DataSize;
use serde::Serialize;

use crate::types::NodeId;

#[derive(Clone, DataSize, Debug, PartialEq, Serialize)]
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
