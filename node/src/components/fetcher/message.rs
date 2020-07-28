use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use super::Item;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(bound = "for<'a> T: Deserialize<'a>")]
pub enum Message<T: Item> {
    /// Requesting item from peer.
    GetRequest(T::Id),
    /// Received item from peer.
    GetResponse(Box<T>),
}

impl<T: Item> Display for Message<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::GetRequest(id) => write!(formatter, "get-request({})", id),
            Message::GetResponse(item) => write!(formatter, "get-response({})", item.id()),
        }
    }
}
