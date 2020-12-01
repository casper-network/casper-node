use std::fmt::{self, Debug, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::crypto::hash::Digest;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message<P> {
    Handshake { genesis_config_hash: Digest },
    Payload(P),
}

impl<P: Display> Display for Message<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Handshake {
                genesis_config_hash,
            } => write!(f, "handshake: {}", genesis_config_hash),
            Message::Payload(payload) => write!(f, "payload: {}", payload),
        }
    }
}
