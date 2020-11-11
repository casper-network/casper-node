use std::fmt::{self, Display, Formatter};

use derive_more::From;
use serde::Serialize;

use super::{StorageType, Value};
use crate::{effect::requests::StorageRequest, types::NodeId};

/// `Storage` events.
#[derive(Debug, From, Serialize)]
pub enum Event<S: StorageType + 'static> {
    /// We received a `GetRequest` message for a `Deploy` from a peer.
    #[serde(skip_serializing)]
    GetDeployForPeer {
        deploy_hash: <S::Deploy as Value>::Id,
        peer: NodeId,
    },
    #[serde(skip_serializing)]
    #[from]
    Request(StorageRequest<S>),
}

impl<S: StorageType + 'static> Display for Event<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::GetDeployForPeer { deploy_hash, peer } => {
                write!(formatter, "get deploy {} for {}", deploy_hash, peer)
            }
            Event::Request(request) => write!(formatter, "{}", request),
        }
    }
}
