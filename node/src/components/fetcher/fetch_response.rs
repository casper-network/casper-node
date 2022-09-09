use serde::{Deserialize, Serialize};

/// Message to be returned by a peer. Indicates if the item could be fetched or not.
#[derive(Serialize, Deserialize)]
pub enum FetchResponse<T, Id> {
    /// The requested item.
    Fetched(T),
    /// The sender does not have the requested item available.
    NotFound(Id),
    /// The sender chose to not provide the requested item.
    NotProvided(Id),
}

impl<T, Id> FetchResponse<T, Id> {
    /// Constructs a fetched or not found from an option and an id.
    pub(crate) fn from_opt(id: Id, item: Option<T>) -> Self {
        match item {
            Some(item) => FetchResponse::Fetched(item),
            None => FetchResponse::NotFound(id),
        }
    }

    /// Returns whether this response is a positive (fetched / "found") one.
    pub(crate) fn was_found(&self) -> bool {
        matches!(self, FetchResponse::Fetched(_))
    }
}

impl<T, Id> FetchResponse<T, Id>
where
    Self: Serialize,
{
    /// The canonical serialization for the inner encoding of the `FetchResponse` response (see
    /// [`Message::GetResponse`]).
    pub(crate) fn to_serialized(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }
}
