use std::fmt::{self, Display, Formatter};

use crate::{
    components::{deploy_fetcher::Message, storage::Result},
    effect::{requests::DeployFetcherRequest, Responder},
    small_network::NodeId,
    types::{Deploy, DeployHash},
};

pub(crate) type DeployResponder = Responder<Option<Box<Deploy>>>;

#[derive(Debug, PartialEq)]
pub enum RequestDirection {
    Inbound,
    Outbound,
}

/// `DeployFetcher` events.
#[derive(Debug)]
pub enum Event {
    /// The initiating event to get a `Deploy` by `DeployHash`
    FetchDeploy {
        deploy_hash: DeployHash,
        peer: NodeId,
        responder: DeployResponder,
    },
    /// The result of the `DeployFetcher` getting a deploy from the storage component.  If the
    /// result is not `Ok`, the deploy should be requested from the peer.
    GetFromStoreResult {
        request_direction: RequestDirection,
        deploy_hash: DeployHash,
        peer: NodeId,
        result: Box<Result<Deploy>>,
    },
    /// The timeout for waiting for the full deploy body has elapsed and we should clean up
    /// state.
    TimeoutPeer {
        deploy_hash: DeployHash,
        peer: NodeId,
    },
    /// An incoming gossip network message.
    MessageReceived { sender: NodeId, message: Message },
    /// The result of the `DeployFetcher` putting a deploy to the storage component.
    PutToStoreResult {
        deploy: Deploy,
        peer: NodeId,
        result: Result<bool>,
    },
}

impl From<DeployFetcherRequest<NodeId>> for Event {
    fn from(request: DeployFetcherRequest<NodeId>) -> Self {
        match request {
            DeployFetcherRequest::FetchDeploy {
                hash,
                peer,
                responder,
            } => Event::FetchDeploy {
                deploy_hash: hash,
                peer,
                responder,
            },
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::TimeoutPeer { deploy_hash, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                deploy_hash, peer
            ),
            Event::FetchDeploy { deploy_hash, .. } => {
                write!(formatter, "request to get deploy at hash {}", deploy_hash)
            }
            Event::MessageReceived { sender, message } => {
                write!(formatter, "{} received from {}", message, sender)
            }
            Event::PutToStoreResult { deploy, result, .. } => {
                if result.is_ok() {
                    write!(formatter, "put {} to store", *deploy.id())
                } else {
                    write!(formatter, "failed to put {} to store", *deploy.id())
                }
            }
            Event::GetFromStoreResult {
                deploy_hash,
                result,
                ..
            } => {
                if result.is_ok() {
                    write!(formatter, "got {} from store", deploy_hash)
                } else {
                    write!(formatter, "failed to get {} from store", deploy_hash)
                }
            }
        }
    }
}
