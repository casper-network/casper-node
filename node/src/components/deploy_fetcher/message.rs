use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::types::{Deploy, DeployHash};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message {
    /// Requesting `Deploy`.
    GetRequest(DeployHash),
    /// Received `Deploy` from peer.
    GetResponse(Box<Deploy>),
}

impl Display for Message {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::GetRequest(deploy_hash) => write!(formatter, "get-request({})", deploy_hash),
            Message::GetResponse(deploy) => write!(formatter, "get-response({})", deploy.id()),
        }
    }
}
