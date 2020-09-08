//! A network message type used for communication between nodes

use std::fmt::{self, Display, Formatter};

use derive_more::From;
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};

use crate::{
    components::{consensus, gossiper, small_network::GossipedAddress},
    types::{Deploy, Item, Tag},
};

/// Reactor message.
#[derive(Debug, Clone, From, Serialize, Deserialize)]
pub enum Message {
    /// Consensus component message.
    #[from]
    Consensus(consensus::ConsensusMessage),
    /// Deploy gossiper component message.
    #[from]
    DeployGossiper(gossiper::Message<Deploy>),
    /// Address gossiper component message.
    #[from]
    AddressGossiper(gossiper::Message<GossipedAddress>),
    /// Request to get an item from a peer.
    GetRequest {
        /// The type tag of the requested item.
        tag: Tag,
        /// The serialized ID of the requested item.
        serialized_id: Vec<u8>,
    },
    /// Response to a `GetRequest`.
    GetResponse {
        /// The type tag of the contained item.
        tag: Tag,
        /// The serialized item.
        serialized_item: Vec<u8>,
    },
}

impl Message {
    pub(crate) fn new_get_request<T: Item>(id: &T::Id) -> Result<Self, rmp_serde::encode::Error> {
        Ok(Message::GetRequest {
            tag: T::TAG,
            serialized_id: rmp_serde::to_vec(id)?,
        })
    }

    pub(crate) fn new_get_response<T: Item>(item: &T) -> Result<Self, rmp_serde::encode::Error> {
        Ok(Message::GetResponse {
            tag: T::TAG,
            serialized_item: rmp_serde::to_vec(item)?,
        })
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Message::Consensus(consensus) => write!(f, "Consensus::{}", consensus),
            Message::DeployGossiper(deploy) => write!(f, "DeployGossiper::{}", deploy),
            Message::AddressGossiper(gossiped_address) => {
                write!(f, "AddressGossiper::({})", gossiped_address)
            }
            Message::GetRequest { tag, serialized_id } => {
                write!(f, "GetRequest({}-{:10})", tag, HexFmt(serialized_id))
            }
            Message::GetResponse {
                tag,
                serialized_item,
            } => write!(f, "GetResponse({}-{:10})", tag, HexFmt(serialized_item)),
        }
    }
}
