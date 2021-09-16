//! Announcements of incoming network messages.
//!
//! Any event suffixed -`Incoming` is usually the arrival of a specific network message.

use std::fmt::Display;

use datasize::DataSize;
use serde::Serialize;

use crate::{
    components::{consensus, gossiper},
    types::{FinalitySignature, NodeId},
};

/// An envelope for an incoming message, attaching a sender address.
#[derive(DataSize, Debug, Serialize)]
pub struct MessageIncoming<I, M> {
    pub(crate) sender: I,
    pub(crate) message: M,
}

impl<I, M> Display for MessageIncoming<I, M>
where
    I: Display,
    M: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "incoming from {}: {}", self.sender, self.message)
    }
}

/// A new consensus message arrived.
pub(crate) type ConsensusMessageIncoming<I> = MessageIncoming<I, consensus::ConsensusMessage>;

/// A new message from a gossiper arrived.
pub type GossiperIncoming<T> = MessageIncoming<NodeId, gossiper::Message<T>>;

/// A new message requesting various objects arrived.
pub(crate) type NetRequestIncoming = MessageIncoming<NodeId, NetRequest>;

/// A new message requesting a trie arrived.
pub(crate) type TrieRequestIncoming = MessageIncoming<NodeId, TrieRequest>;

/// A new message requesting various objects arrived.
pub(crate) type NetResponseIncoming = MessageIncoming<NodeId, NetResponse>;

/// A new message requesting a trie arrived.
pub(crate) type TrieResponseIncoming = MessageIncoming<NodeId, TrieResponse>;

/// A new finality signature arrived over the network.
pub(crate) type FinalitySignatureIncoming = MessageIncoming<NodeId, Box<FinalitySignature>>;

/// A request for an object out of storage arrived.
///
/// Note: The variants here are grouped under a common enum, since they are usually handled by the
///       same component. If this changes, split up this type (see `TrieRequestIncoming` for an
///       example).
#[derive(DataSize, Debug, Serialize)]
pub(crate) enum NetRequest {
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

impl Display for NetRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetRequest::Deploy(_) => f.write_str("request for deploy"),
            NetRequest::Block(_) => f.write_str("request for block"),
            NetRequest::GossipedAddress(_) => f.write_str("request for gossiped address"),
            NetRequest::BlockAndMetadataByHeight(_) => {
                f.write_str("request for block and metadata by height")
            }
            NetRequest::BlockHeaderByHash(_) => f.write_str("request for block header by hash"),
            NetRequest::BlockHeaderAndFinalitySignaturesByHeight(_) => {
                f.write_str("request for block header and finality signatures by height")
            }
        }
    }
}

/// A request for a trie.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct TrieRequest(pub(crate) Vec<u8>);

impl Display for TrieRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("request for trie")
    }
}

/// A response for a net request.
///
/// See `NetRequest` for notes.
#[derive(DataSize, Debug, Serialize)]
pub(crate) enum NetResponse {
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

impl Display for NetResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetResponse::Deploy(_) => f.write_str("response, deploy"),
            NetResponse::Block(_) => f.write_str("response, block"),
            NetResponse::GossipedAddress(_) => f.write_str("response, gossiped address"),
            NetResponse::BlockAndMetadataByHeight(_) => {
                f.write_str("response, block and metadata by height")
            }
            NetResponse::BlockHeaderByHash(_) => f.write_str("response, block header by hash"),
            NetResponse::BlockHeaderAndFinalitySignaturesByHeight(_) => {
                f.write_str("response, block header and finality signatures by height")
            }
        }
    }
}

/// A response to a request for a trie.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct TrieResponse(pub(crate) Vec<u8>);

impl Display for TrieResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("response, trie")
    }
}
