use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::types::{Deploy, DeployHash};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message {
    /// Gossiped out to random peers to notify them of a `Deploy` we hold.
    Gossip(DeployHash),
    /// Response to a `Gossip` message.  If `is_already_held` is false, the recipient should treat
    /// this as a `GetRequest` and send a `GetResponse` containing the `Deploy`.
    GossipResponse {
        deploy_hash: DeployHash,
        is_already_held: bool,
    },
    /// Sent if a `Deploy` fails to arrive, either after sending a `GossipResponse` with
    /// `is_already_held` set to false, or after a previous `GetRequest`.
    GetRequest(DeployHash),
    /// Sent in response to a `GetRequest`, or to a peer which responded to gossip indicating it
    /// didn't already hold the full `Deploy`.
    GetResponse(Box<Deploy>),
}

impl Display for Message {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Gossip(deploy_hash) => write!(formatter, "gossip({})", deploy_hash),
            Message::GossipResponse {
                deploy_hash,
                is_already_held,
            } => write!(
                formatter,
                "gossip-response({}, {})",
                deploy_hash, is_already_held
            ),
            Message::GetRequest(deploy_hash) => write!(formatter, "get-request({})", deploy_hash),
            Message::GetResponse(deploy) => write!(formatter, "get-response({})", deploy.id()),
        }
    }
}
