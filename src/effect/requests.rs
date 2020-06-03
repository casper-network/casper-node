use std::fmt::{self, Debug, Display, Formatter};

use super::Responder;
use crate::components::storage::{StorageType, Store, Value};

type ResultResponder<T, E> = Responder<Result<T, E>>;

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
        block: <S::BlockStore as Store>::Value,
        responder: Responder<bool>,
    },
    /// Retrieve block with given hash.
    GetBlock {
        block_hash: <<S::BlockStore as Store>::Value as Value>::Id,
        responder:
            ResultResponder<<S::BlockStore as Store>::Value, <S::BlockStore as Store>::Error>,
    },
    /// Retrieve block header with given hash.
    GetBlockHeader {
        block_hash: <<S::BlockStore as Store>::Value as Value>::Id,
        responder: Responder<Option<<<S::BlockStore as Store>::Value as Value>::Header>>,
    },
    /// Store given deploy.
    PutDeploy {
        deploy: <S::DeployStore as Store>::Value,
        responder: Responder<bool>,
    },
    /// Retrieve deploy with given hash.
    GetDeploy {
        deploy_hash: <<S::DeployStore as Store>::Value as Value>::Id,
        responder:
            ResultResponder<<S::DeployStore as Store>::Value, <S::DeployStore as Store>::Error>,
    },
    /// Retrieve deploy header with given hash.
    GetDeployHeader {
        deploy_hash: <<S::DeployStore as Store>::Value as Value>::Id,
        responder: Responder<Option<<<S::DeployStore as Store>::Value as Value>::Header>>,
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
            StorageRequest::GetBlockHeader {
                block_hash,
                responder,
            } => write!(
                formatter,
                "StorageRequest::GetBlockHeader {{ block_hash: {:?}, responder: {:?} }}",
                block_hash, responder
            ),
            StorageRequest::PutDeploy { deploy, responder } => write!(
                formatter,
                "StorageRequest::PutDeploy {{ deploy: {:?}, responder: {:?} }}",
                deploy, responder
            ),
            StorageRequest::GetDeploy {
                deploy_hash,
                responder,
            } => write!(
                formatter,
                "StorageRequest::GetDeploy {{ deploy_hash: {:?}, responder: {:?} }}",
                deploy_hash, responder
            ),
            StorageRequest::GetDeployHeader {
                deploy_hash,
                responder,
            } => write!(
                formatter,
                "StorageRequest::GetDeployHeader {{ deploy_hash: {:?}, responder: {:?} }}",
                deploy_hash, responder
            ),
        }
    }
}
