use std::fmt::{self, Debug, Display, Formatter};

use super::Responder;
use crate::{
    components::storage::{StorageType, Store, Value},
    types::{Deploy, DeployHash},
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

impl<I, P> NetworkRequest<I, P> {
    /// Transform a network request by mapping the contained payload.
    ///
    /// This is a replacement for a `From` conversion that is not possible without specialization.
    pub(crate) fn map_payload<F, P2>(self, wrap_payload: F) -> NetworkRequest<I, P2>
    where
        F: FnOnce(P) -> P2,
    {
        match self {
            NetworkRequest::SendMessage {
                dest,
                payload,
                responder,
            } => NetworkRequest::SendMessage {
                dest,
                payload: wrap_payload(payload),
                responder,
            },
            NetworkRequest::BroadcastMessage { payload, responder } => {
                NetworkRequest::BroadcastMessage {
                    payload: wrap_payload(payload),
                    responder,
                }
            }
        }
    }
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

#[derive(Debug)]
#[allow(clippy::type_complexity)]
// TODO: remove once all variants are used.
#[allow(dead_code)]
pub(crate) enum StorageRequest<S: StorageType> {
    /// Store given block.
    PutBlock {
        block: Box<<S::BlockStore as Store>::Value>,
        responder: Responder<Result<(), <S::BlockStore as Store>::Error>>,
    },
    /// Retrieve block with given hash.
    GetBlock {
        block_hash: <<S::BlockStore as Store>::Value as Value>::Id,
        responder:
            Responder<Result<<S::BlockStore as Store>::Value, <S::BlockStore as Store>::Error>>,
    },
    /// Retrieve block header with given hash.
    GetBlockHeader {
        block_hash: <<S::BlockStore as Store>::Value as Value>::Id,
        responder: Responder<
            Result<
                <<S::BlockStore as Store>::Value as Value>::Header,
                <S::BlockStore as Store>::Error,
            >,
        >,
    },
    /// Store given deploy.
    PutDeploy {
        deploy: Box<<S::DeployStore as Store>::Value>,
        responder: Responder<Result<(), <S::DeployStore as Store>::Error>>,
    },
    /// Retrieve deploy with given hash.
    GetDeploy {
        deploy_hash: <<S::DeployStore as Store>::Value as Value>::Id,
        responder:
            Responder<Result<<S::DeployStore as Store>::Value, <S::DeployStore as Store>::Error>>,
    },
    /// Retrieve deploy header with given hash.
    GetDeployHeader {
        deploy_hash: <<S::DeployStore as Store>::Value as Value>::Id,
        responder: Responder<
            Result<
                <<S::DeployStore as Store>::Value as Value>::Header,
                <S::DeployStore as Store>::Error,
            >,
        >,
    },
}

impl<S: StorageType> Display for StorageRequest<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StorageRequest::PutBlock { block, .. } => write!(formatter, "put {}", block),
            StorageRequest::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
            StorageRequest::GetBlockHeader { block_hash, .. } => {
                write!(formatter, "get {}", block_hash)
            }
            StorageRequest::PutDeploy { deploy, .. } => write!(formatter, "put {}", deploy),
            StorageRequest::GetDeploy { deploy_hash, .. } => {
                write!(formatter, "get {}", deploy_hash)
            }
            StorageRequest::GetDeployHeader { deploy_hash, .. } => {
                write!(formatter, "get {}", deploy_hash)
            }
        }
    }
}

/// Abstract API request
///
/// An API request is an abstract request that does not concern itself with serialization or
/// transport.
#[derive(Debug)]
pub(crate) enum ApiRequest {
    /// Submit a deploy for storing.
    ///
    /// Returns the deploy along with an error message if it could not be stored.
    SubmitDeploy {
        deploy: Box<Deploy>,
        responder: Responder<Result<(), (Deploy, String)>>,
    },
    /// Return the specified deploy if it exists, else `None`.
    GetDeploy {
        hash: DeployHash,
        responder: Responder<Option<Deploy>>,
    },
}

impl Display for ApiRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ApiRequest::SubmitDeploy { deploy, .. } => write!(formatter, "submit {}", *deploy),
            ApiRequest::GetDeploy { hash, .. } => write!(formatter, "get {}", hash),
        }
    }
}
