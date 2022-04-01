use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use super::Item;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(bound = "for<'a> T: Deserialize<'a>")]
pub(crate) enum Message<T: Item> {
    /// Gossiped out to random peers to notify them of an item we hold.
    Gossip(T::Id),
    /// Response to a `Gossip` message.  If `is_already_held` is false, the recipient should treat
    /// this as a `GetRequest` and send a `GetResponse` containing the item.
    GossipResponse {
        item_id: T::Id,
        is_already_held: bool,
    },
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
        }
    }
}
