//! Announcements of incoming network messages.
//!
//! Any event suffixed -`Incoming` is usually the arrival of a specific network message.

use std::{fmt::Display, sync::Arc};

use datasize::DataSize;
use serde::Serialize;

use crate::{
    components::{consensus, gossiper},
    protocol::Message,
    types::{FinalitySignature, NodeId, Tag, TrieOrChunkIdDisplay},
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

/// A new message responding to a request arrived.
pub(crate) type NetResponseIncoming = MessageIncoming<NetResponse>;

/// A new message requesting a trie arrived.
pub(crate) type TrieRequestIncoming = MessageIncoming<TrieRequest>;

/// A demand for a try that should be answered.
pub(crate) type TrieDemand = DemandIncoming<TrieRequest>;

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
    Deploy(Vec<u8>),
    LegacyDeploy(Vec<u8>),
    BlockDeployApprovals(Vec<u8>),
    Block(Vec<u8>),
    // TODO: Move this out of `NetRequest` into its own type, it is never valid.
    FinalitySignature(Vec<u8>),
    // TODO: Move this out of `NetRequest` into its own type, it is never valid.
    GossipedAddress(Vec<u8>),
    BlockAndDeploys(Vec<u8>),
    BlockHeaderByHash(Vec<u8>),
    BlockHeadersBatch(Vec<u8>),
    FinalitySignatures(Vec<u8>),
    SyncLeap(Vec<u8>),
    BlockExecutionResults(Vec<u8>),
    ExecutedBlock(Vec<u8>),
}

impl Display for NetRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetRequest::Deploy(_) => f.write_str("request for deploy"),
            NetRequest::LegacyDeploy(_) => f.write_str("request for legacy deploy"),
            NetRequest::BlockDeployApprovals(_) => f.write_str("request for finalized approvals"),
            NetRequest::Block(_) => f.write_str("request for block"),
            NetRequest::FinalitySignature(_) => {
                f.write_str("request for gossiped finality signature")
            }
            NetRequest::GossipedAddress(_) => f.write_str("request for gossiped address"),
            NetRequest::BlockHeaderByHash(_) => f.write_str("request for block header by hash"),
            NetRequest::BlockAndDeploys(_) => f.write_str("request for a block and its deploys"),
            NetRequest::BlockHeadersBatch(_) => f.write_str("request for block headers batch"),
            NetRequest::FinalitySignatures(_) => f.write_str("request for finality signatures"),
            NetRequest::SyncLeap(_) => f.write_str("request for sync leap"),
            NetRequest::BlockExecutionResults(_) => {
                f.write_str("request for block execution results")
            }
            NetRequest::ExecutedBlock(_) => f.write_str("request for executed block"),
        }
    }
}

impl NetRequest {
    /// Returns a unique identifier of the requested object.
    pub(crate) fn unique_id(&self) -> Vec<u8> {
        let id = match self {
            NetRequest::Deploy(ref id) => id,
            NetRequest::LegacyDeploy(ref id) => id,
            NetRequest::BlockDeployApprovals(ref id) => id,
            NetRequest::Block(ref id) => id,
            NetRequest::FinalitySignature(ref id) => id,
            NetRequest::GossipedAddress(ref id) => id,
            NetRequest::BlockHeaderByHash(ref id) => id,
            NetRequest::BlockAndDeploys(ref id) => id,
            NetRequest::BlockHeadersBatch(ref id) => id,
            NetRequest::FinalitySignatures(ref id) => id,
            NetRequest::SyncLeap(ref id) => id,
            NetRequest::BlockExecutionResults(ref id) => id,
            NetRequest::ExecutedBlock(ref id) => id,
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
            NetRequest::LegacyDeploy(_) => Tag::LegacyDeploy,
            NetRequest::BlockDeployApprovals(_) => Tag::BlockDeployApprovals,
            NetRequest::Block(_) => Tag::Block,
            NetRequest::FinalitySignature(_) => Tag::FinalitySignature,
            NetRequest::GossipedAddress(_) => Tag::GossipedAddress,
            NetRequest::BlockHeaderByHash(_) => Tag::BlockHeaderByHash,
            NetRequest::BlockAndDeploys(_) => Tag::BlockAndDeploysByHash,
            NetRequest::BlockHeadersBatch(_) => Tag::BlockHeaderBatch,
            NetRequest::FinalitySignatures(_) => Tag::FinalitySignaturesByHash,
            NetRequest::SyncLeap(_) => Tag::SyncLeap,
            NetRequest::BlockExecutionResults(_) => Tag::BlockExecutionResults,
            NetRequest::ExecutedBlock(_) => Tag::ApprovalsHashes,
        }
    }
}

/// A response for a net request.
///
/// See `NetRequest` for notes.
#[derive(Debug, Serialize)]
pub(crate) enum NetResponse {
    Deploy(Arc<[u8]>),
    LegacyDeploy(Arc<[u8]>),
    FinalizedApprovals(Arc<[u8]>),
    Block(Arc<[u8]>),
    GossipedAddress(Arc<[u8]>),
    BlockHeaderByHash(Arc<[u8]>),
    BlockAndDeploys(Arc<[u8]>),
    BlockHeadersBatch(Arc<[u8]>),
    FinalitySignature(Arc<[u8]>),
    FinalitySignatures(Arc<[u8]>),
    SyncLeap(Arc<[u8]>),
    BlockExecutionResults(Arc<[u8]>),
    ExecutedBlock(Arc<[u8]>),
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
            NetResponse::LegacyDeploy(_) => f.write_str("response, legacy deploy"),
            NetResponse::FinalizedApprovals(_) => f.write_str("response, finalized approvals"),
            NetResponse::Block(_) => f.write_str("response, block"),
            NetResponse::FinalitySignature(_) => f.write_str("response, finality signature"),
            NetResponse::GossipedAddress(_) => f.write_str("response, gossiped address"),
            NetResponse::BlockHeaderByHash(_) => f.write_str("response, block header by hash"),
            NetResponse::BlockAndDeploys(_) => f.write_str("response, block and deploys"),
            NetResponse::BlockHeadersBatch(_) => f.write_str("response for block-headers-batch"),
            NetResponse::FinalitySignatures(_) => f.write_str("response for finality signatures"),
            NetResponse::SyncLeap(_) => f.write_str("response for sync leap"),
            NetResponse::BlockExecutionResults(_) => {
                f.write_str("response for block execution results")
            }
            NetResponse::ExecutedBlock(_) => f.write_str("response for executed block"),
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

        let a = NetRequest::Deploy(inner_id.clone());
        let b = NetRequest::Block(inner_id);

        assert_ne!(a.unique_id(), b.unique_id());
    }
}
