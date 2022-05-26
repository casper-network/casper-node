//! Announcements of incoming network messages.
//!
//! Any event suffixed -`Incoming` is usually the arrival of a specific network message.

use std::{fmt::Display, sync::Arc};

use casper_execution_engine::storage::trie::TrieOrChunkIdDisplay;
use datasize::DataSize;
use serde::Serialize;

use crate::{
    components::{consensus, gossiper},
    protocol::Message,
    types::{FinalitySignature, NodeId, Tag},
};

use super::AutoClosingResponder;

/// An envelope for an incoming message, attaching a sender address.
#[derive(DataSize, Debug, Serialize)]
pub struct MessageIncoming<M> {
    pub(crate) sender: NodeId,
    pub(crate) message: M,
}

impl<M> Display for MessageIncoming<M>
where
    M: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "incoming from {}: {}", self.sender, self.message)
    }
}

/// An envelope for an incoming demand, attaching a sender address and responder.
#[derive(DataSize, Debug, Serialize)]
pub struct DemandIncoming<M> {
    /// The sender from which the demand originated.
    pub(crate) sender: NodeId,
    /// The wrapped demand.
    pub(crate) request_msg: M,
    /// Responder to send the answer down through.
    pub(crate) auto_closing_responder: AutoClosingResponder<Message>,
}

impl<M> Display for DemandIncoming<M>
where
    M: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "demand from {}: {}", self.sender, self.request_msg)
    }
}

/// A new consensus message arrived.
pub(crate) type ConsensusMessageIncoming = MessageIncoming<consensus::ConsensusMessage>;

/// A new message from a gossiper arrived.
pub(crate) type GossiperIncoming<T> = MessageIncoming<gossiper::Message<T>>;

/// A new message requesting various objects arrived.
pub(crate) type NetRequestIncoming = MessageIncoming<NetRequest>;

/// A new message requesting a trie arrived.
pub(crate) type TrieRequestIncoming = MessageIncoming<TrieRequest>;

/// A demand for a try that should be answered.
pub(crate) type TrieDemand = DemandIncoming<TrieRequest>;

/// A new message responding to a request arrived.
pub(crate) type NetResponseIncoming = MessageIncoming<NetResponse>;

/// A new message responding to a trie request arrived.
pub(crate) type TrieResponseIncoming = MessageIncoming<TrieResponse>;

/// A new finality signature arrived over the network.
pub(crate) type FinalitySignatureIncoming = MessageIncoming<Box<FinalitySignature>>;

/// A request for an object out of storage arrived.
///
/// Note: The variants here are grouped under a common enum, since they are usually handled by the
///       same component. If this changes, split up this type (see `TrieRequestIncoming` for an
///       example).
#[derive(DataSize, Debug, Serialize)]
#[repr(u8)]
pub(crate) enum NetRequest {
    /// Request for a deploy.
    Deploy(Vec<u8>),
    /// Request for finalized approvals.
    FinalizedApprovals(Vec<u8>),
    /// Request for a block.
    Block(Vec<u8>),
    /// Request for a gossiped public listening address.
    // TODO: Move this out of `NetRequest` into its own type, it is never valid.
    GossipedAddress(Vec<u8>),
    /// Request for a block by its height in the linear chain.
    BlockAndMetadataByHeight(Vec<u8>),
    /// Request for a block and its deploys by hash.
    BlockAndDeploys(Vec<u8>),
    /// Request for a block header by its hash.
    BlockHeaderByHash(Vec<u8>),
    /// Request for a block header and its finality signatures by its height in the linear chain.
    BlockHeaderAndFinalitySignaturesByHeight(Vec<u8>),
}

impl Display for NetRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetRequest::Deploy(_) => f.write_str("request for deploy"),
            NetRequest::FinalizedApprovals(_) => f.write_str("request for finalized approvals"),
            NetRequest::Block(_) => f.write_str("request for block"),
            NetRequest::GossipedAddress(_) => f.write_str("request for gossiped address"),
            NetRequest::BlockAndMetadataByHeight(_) => {
                f.write_str("request for block and metadata by height")
            }
            NetRequest::BlockHeaderByHash(_) => f.write_str("request for block header by hash"),
            NetRequest::BlockHeaderAndFinalitySignaturesByHeight(_) => {
                f.write_str("request for block header and finality signatures by height")
            }
            NetRequest::BlockAndDeploys(_) => f.write_str("request for a block and its deploys"),
        }
    }
}

impl NetRequest {
    /// Returns a unique identifier of the requested object.
    ///
    /// The identifier is based on the domain-specific ID and the tag of the item, thus
    /// `BlockAndMetadataByHeight` and `BlockHeaderAndFinalitySignaturesByHeight` have different IDs
    /// even if the same height was requested.
    pub(crate) fn unique_id(&self) -> Vec<u8> {
        let id = match self {
            NetRequest::Deploy(ref id) => id,
            NetRequest::FinalizedApprovals(ref id) => id,
            NetRequest::Block(ref id) => id,
            NetRequest::GossipedAddress(ref id) => id,
            NetRequest::BlockAndMetadataByHeight(ref id) => id,
            NetRequest::BlockHeaderByHash(ref id) => id,
            NetRequest::BlockHeaderAndFinalitySignaturesByHeight(ref id) => id,
            NetRequest::BlockAndDeploys(ref id) => id,
        };
        let mut unique_id = Vec::with_capacity(id.len() + 1);
        unique_id.push(self.tag() as u8);
        unique_id.extend(id);

        unique_id
    }

    /// Returns the tag associated with the request.
    pub(crate) fn tag(&self) -> Tag {
        match self {
            NetRequest::Deploy(_) => Tag::Deploy,
            NetRequest::FinalizedApprovals(_) => Tag::FinalizedApprovals,
            NetRequest::Block(_) => Tag::Block,
            NetRequest::GossipedAddress(_) => Tag::GossipedAddress,
            NetRequest::BlockAndMetadataByHeight(_) => Tag::BlockAndMetadataByHeight,
            NetRequest::BlockHeaderByHash(_) => Tag::BlockHeaderByHash,
            NetRequest::BlockHeaderAndFinalitySignaturesByHeight(_) => {
                Tag::BlockHeaderAndFinalitySignaturesByHeight
            }
            NetRequest::BlockAndDeploys(_) => Tag::BlockAndDeploysByHash,
        }
    }
}

/// A request for a trie.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct TrieRequest(pub(crate) Vec<u8>);

impl Display for TrieRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "request for trie {}", TrieOrChunkIdDisplay(&self.0))
    }
}

/// A response for a net request.
///
/// See `NetRequest` for notes.
#[derive(Debug, Serialize)]
pub(crate) enum NetResponse {
    /// Response of a deploy.
    Deploy(Arc<[u8]>),
    /// Response of finalized approvals.
    FinalizedApprovals(Arc<[u8]>),
    /// Response of a block.
    Block(Arc<[u8]>),
    /// Response of a gossiped public listening address.
    GossipedAddress(Arc<[u8]>),
    /// Response of a block by its height in the linear chain.
    BlockAndMetadataByHeight(Arc<[u8]>),
    /// Response of a block header by its hash.
    BlockHeaderByHash(Arc<[u8]>),
    /// Response of a block header and its finality signatures by its height in the linear chain.
    BlockHeaderAndFinalitySignaturesByHeight(Arc<[u8]>),
    /// Response for a block and its deploys.
    BlockAndDeploys(Arc<[u8]>),
}

// `NetResponse` uses `Arcs`, so we count all data as 0.
impl DataSize for NetResponse {
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

impl Display for NetResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetResponse::Deploy(_) => f.write_str("response, deploy"),
            NetResponse::FinalizedApprovals(_) => f.write_str("response, finalized approvals"),
            NetResponse::Block(_) => f.write_str("response, block"),
            NetResponse::GossipedAddress(_) => f.write_str("response, gossiped address"),
            NetResponse::BlockAndMetadataByHeight(_) => {
                f.write_str("response, block and metadata by height")
            }
            NetResponse::BlockHeaderByHash(_) => f.write_str("response, block header by hash"),
            NetResponse::BlockHeaderAndFinalitySignaturesByHeight(_) => {
                f.write_str("response, block header and finality signatures by height")
            }
            NetResponse::BlockAndDeploys(_) => f.write_str("response, block and deploys"),
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

#[cfg(test)]
mod tests {
    use super::NetRequest;

    #[test]
    fn unique_id_is_unique_across_variants() {
        let inner_id = b"example".to_vec();

        let a = NetRequest::BlockAndMetadataByHeight(inner_id.clone());
        let b = NetRequest::BlockHeaderAndFinalitySignaturesByHeight(inner_id);

        assert_ne!(a.unique_id(), b.unique_id());
    }
}
