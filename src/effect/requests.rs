use std::fmt::{self, Debug, Display, Formatter};

use super::Responder;
use crate::{
    components::{
        api_server::Deploy,
        storage::{BlockStoreType, BlockType, StorageType},
    },
    utils::DisplayIter,
};

#[derive(Debug)]
pub(crate) enum NetworkRequest<I, P> {
    /// Send a message on the network to a specific peer.
    SendMessage {
        dest: I,
        payload: P,
        responder: Responder<()>,
    },
    /// Send a message on the network to all peers.
    BroadcastMessage {
        payload: P,
        responder: Responder<()>,
    },
}

impl<I, P> Display for NetworkRequest<I, P>
where
    I: Display,
    P: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkRequest::SendMessage { dest, payload, .. } => {
                write!(formatter, "send to {}: {}", dest, payload)
            }
            NetworkRequest::BroadcastMessage { payload, .. } => {
                write!(formatter, "broadcast: {}", payload)
            }
        }
    }
}

pub(crate) enum StorageRequest<S: StorageType> {
    /// Store given block.
    PutBlock {
        block: <S::BlockStore as BlockStoreType>::Block,
        responder: Responder<bool>,
    },
    /// Retrieve block with given hash.
    GetBlock {
        block_hash: <<S::BlockStore as BlockStoreType>::Block as BlockType>::Hash,
        responder: Responder<Option<<S::BlockStore as BlockStoreType>::Block>>,
    },
}

impl<S: StorageType> Display for StorageRequest<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, .. } => write!(formatter, "put {}", block),
            StorageRequest::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
        }
    }
}

impl<S: StorageType> Debug for StorageRequest<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, responder } => write!(
                formatter,
                "StorageRequest::PutBlock {{ block: {:?}, responder: {:?} }}",
                block, responder
            ),
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => write!(
                formatter,
                "StorageRequest::GetBlock {{ block_hash: {:?}, responder: {:?} }}",
                block_hash, responder
            ),
        }
    }
}

/// Abstract API request
///
/// An API request is an abstract request that does not concern itself with serialization or
/// transport.
#[derive(Debug)]
pub(crate) enum ApiRequest {
    /// Return a list of deploys.
    GetDeploys { responder: Responder<Vec<Deploy>> },
    /// Submit a number of deploys for inclusion.
    ///
    /// Returns the deploys that could not be stored.
    SubmitDeploys {
        responder: Responder<Vec<Deploy>>,
        deploys: Vec<Deploy>,
    },
}

impl Display for ApiRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ApiRequest::GetDeploys { .. } => write!(formatter, "get deploys"),
            ApiRequest::SubmitDeploys { deploys, .. } => write!(
                formatter,
                "submit deploys: {:10}",
                DisplayIter::new(deploys.iter())
            ),
        }
    }
}
