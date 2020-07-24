use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use super::Item;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(bound = "for<'a> T: Deserialize<'a>")]
pub enum Message<T: Item> {
    /// Gossiped out to random peers to notify them of an item we hold.
    Gossip(T::Id),
    /// Response to a `Gossip` message.  If `is_already_held` is false, the recipient should treat
    /// this as a `GetRequest` and send a `GetResponse` containing the item.
    GossipResponse {
        item_id: T::Id,
        is_already_held: bool,
    },
    /// Sent if an item fails to arrive, either after sending a `GossipResponse` with
    /// `is_already_held` set to false, or after a previous `GetRequest`.
    GetRequest(T::Id),
    /// Sent in response to a `GetRequest`, or to a peer which responded to gossip indicating it
    /// didn't already hold the full item.
    GetResponse(Box<T>),
}

impl<T: Item> Display for Message<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Gossip(item_id) => write!(formatter, "gossip({})", item_id),
            Message::GossipResponse {
                item_id,
                is_already_held,
            } => write!(
                formatter,
                "gossip-response({}, {})",
                item_id, is_already_held
            ),
            Message::GetRequest(item_id) => write!(formatter, "get-request({})", item_id),
            Message::GetResponse(item) => write!(formatter, "get-response({})", item.id()),
        }
    }
}
