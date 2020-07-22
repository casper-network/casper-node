use std::fmt::{self, Display, Formatter};

use crate::{
    components::{
        deploy_fetcher::Message,
        storage::Result,
    },
    effect::Responder,
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
        maybe_responder: Option<DeployResponder>,
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
    /// The result of the `DeployFetcher` putting a deploy to the storage component.  If the
    /// result is `Ok`, the deploy hash should be gossiped onwards.
    PutToStoreResult {
        deploy_hash: DeployHash,
        maybe_sender: Option<NodeId>,
        result: Result<bool>,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::TimeoutPeer { deploy_hash, peer } => write!(
                formatter,
                "check get from peer timeout for {} with {}",
                deploy_hash, peer
            ),
            Event::FetchDeploy {
                deploy_hash,
                peer: _,
                maybe_responder: _,
            } => write!(formatter, "request to get deploy at hash {}", deploy_hash),
            Event::MessageReceived { sender, message } => {
                write!(formatter, "{} received from {}", message, sender)
            }
            Event::PutToStoreResult {
                deploy_hash,
                result,
                ..
            } => {
                if result.is_ok() {
                    write!(formatter, "put {} to store", deploy_hash)
                } else {
                    write!(formatter, "failed to put {} to store", deploy_hash)
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
