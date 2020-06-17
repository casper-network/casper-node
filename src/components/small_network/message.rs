use std::{
    collections::HashSet,
    fmt::{self, Debug, Display, Formatter},
};

use serde::{Deserialize, Serialize};

use super::Endpoint;
use crate::{tls::Signed, utils::DisplayIter};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message<P> {
    /// A pruned set of all endpoint announcements the server has received.
    Snapshot(HashSet<Signed<Endpoint>>),
    /// Broadcast a new endpoint known to the sender.
    BroadcastEndpoint(Signed<Endpoint>),
    /// A payload message.
    Payload(P),
}

impl<P: Display> Display for Message<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Snapshot(snapshot) => {
                write!(f, "snapshot: {:10}", DisplayIter::new(snapshot.iter()))
            }
            Message::BroadcastEndpoint(endpoint) => write!(f, "broadcast endpoint: {}", endpoint),
            Message::Payload(payload) => write!(f, "payload: {}", payload),
        }
    }
}
