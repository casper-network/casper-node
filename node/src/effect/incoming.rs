//! Announcements of incoming network messages.
//!
//! Any event suffixed -`Incoming` is usually the arrival of a specific network message.

use std::fmt::Display;

use datasize::DataSize;
use serde::Serialize;

use crate::{
    components::{consensus, gossiper, small_network::GossipedAddress},
    types::{Deploy, FinalitySignature},
};

/// A new consensus message arrived.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct ConsensusMessageIncoming(pub(crate) consensus::ConsensusMessage);

impl Display for ConsensusMessageIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "consensus: {}", self.0)
    }
}

/// A new deploy gossiper message arrived.
#[derive(DataSize, Debug, Serialize)]

pub(crate) struct DeployGossiperIncoming(#[data_size(skip)] pub(crate) gossiper::Message<Deploy>);

impl Display for DeployGossiperIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deploy gossip: {}", self.0)
    }
}

/// A new address gossiper message arrived.
#[derive(DataSize, Debug, Serialize)]

pub(crate) struct AddressGossiperIncoming(
    #[data_size(skip)] pub(crate) gossiper::Message<GossipedAddress>,
);

impl Display for AddressGossiperIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "address gossip: {}", self.0)
    }
}

/// A new request for a object out of storage arrived.
///
/// Note: The variants here are grouped under a common enum, since they are usually handled by the
///       same component. If this changes, split up this type (see `TrieRequestIncoming` for an
///       example).
#[derive(DataSize, Debug, Serialize)]
pub(crate) enum NetRequestIncoming {
    /// Request for a deploy.
    Deploy(Vec<u8>),
    /// Request for a block.
    Block(Vec<u8>),
    /// Request for a gossiped public listening address.
    GossipedAddress(Vec<u8>),
    /// Request for a block by its height in the linear chain.
    BlockAndMetadataByHeight(Vec<u8>),
    /// Request for a block header by its hash.
    BlockHeaderByHash(Vec<u8>),
    /// Request for a block header and its finality signatures by its height in the linear chain.
    BlockHeaderAndFinalitySignaturesByHeight(Vec<u8>),
}

impl Display for NetRequestIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetRequestIncoming::Deploy(inner) => f.write_str("request for deploy"),
            NetRequestIncoming::Block(inner) => f.write_str("request for block"),
            NetRequestIncoming::GossipedAddress(inner) => {
                f.write_str("request for gossiped address")
            }
            NetRequestIncoming::BlockAndMetadataByHeight(inner) => {
                f.write_str("request for block and metadata by height")
            }
            NetRequestIncoming::BlockHeaderByHash(inner) => {
                f.write_str("request for block header by hash")
            }
            NetRequestIncoming::BlockHeaderAndFinalitySignaturesByHeight(inner) => {
                f.write_str("request for block header and finality signatures by height")
            }
        }
    }
}

/// A new request for a trie arrived.
///
/// See `NetRequestIncoming` for notes.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct TrieRequestIncoming(pub(crate) Vec<u8>);

impl Display for TrieRequestIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("request for trie")
    }
}

/// A response for a net request arrived.
///
/// See `NetRequestIncoming` for notes.
#[derive(DataSize, Debug, Serialize)]
pub(crate) enum NetResponseIncoming {
    /// Response of a deploy.
    Deploy(Vec<u8>),
    /// Response of a block.
    Block(Vec<u8>),
    /// Response of a gossiped public listening address.
    GossipedAddress(Vec<u8>),
    /// Response of a block by its height in the linear chain.
    BlockAndMetadataByHeight(Vec<u8>),
    /// Response of a block header by its hash.
    BlockHeaderByHash(Vec<u8>),
    /// Response of a block header and its finality signatures by its height in the linear chain.
    BlockHeaderAndFinalitySignaturesByHeight(Vec<u8>),
}

impl Display for NetResponseIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetResponseIncoming::Deploy(inner) => f.write_str("response, deploy"),
            NetResponseIncoming::Block(inner) => f.write_str("response, block"),
            NetResponseIncoming::GossipedAddress(inner) => {
                f.write_str("response, gossiped address")
            }
            NetResponseIncoming::BlockAndMetadataByHeight(inner) => {
                f.write_str("response, block and metadata by height")
            }
            NetResponseIncoming::BlockHeaderByHash(inner) => {
                f.write_str("response, block header by hash")
            }
            NetResponseIncoming::BlockHeaderAndFinalitySignaturesByHeight(inner) => {
                f.write_str("response, block header and finality signatures by height")
            }
        }
    }
}

/// A response for a trie request arrived.
///
/// See `NetRequestIncoming` for notes.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct TrieResponseIncoming(pub(crate) Vec<u8>);

impl Display for TrieResponseIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("response, trie")
    }
}

/// A new finality signature arrived over the network.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct FinalitySignatureIncoming(pub(crate) Box<FinalitySignature>);

impl Display for FinalitySignatureIncoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("finality signature")
    }
}
