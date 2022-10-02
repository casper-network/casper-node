use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use serde::Serialize;

use crate::types::{FetcherItem, NodeId};

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

    pub(crate) fn convert<U>(self) -> FetchedData<U>
    where
        T: Into<U>,
    {
        match self {
            FetchedData::FromStorage { item } => FetchedData::FromStorage {
                item: Box::new((*item).into()),
            },
            FetchedData::FromPeer { item, peer } => FetchedData::FromPeer {
                item: Box::new((*item).into()),
                peer,
            },
        }
    }
}

impl<T: FetcherItem> Display for FetchedData<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            FetchedData::FromStorage { item } => {
                write!(f, "fetched {} from storage", item.id())
            }
            FetchedData::FromPeer { item, peer } => {
                write!(f, "fetched {} from {}", item.id(), peer)
            }
        }
    }
}
