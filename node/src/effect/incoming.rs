//! Announcements of incoming network messages.
//!
//! Any event suffixed -`Incoming` is usually the arrival of a specific network message.

use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use datasize::DataSize;
use serde::Serialize;

use casper_types::FinalitySignatureV2;

use super::AutoClosingResponder;
use crate::{
    components::{consensus, fetcher::Tag, gossiper},
    protocol::Message,
    types::{NodeId, TrieOrChunkIdDisplay},
};

/// An envelope for an incoming message, attaching a sender address.
#[derive(DataSize, Debug, Serialize)]
pub struct MessageIncoming<M> {
    pub(crate) sender: NodeId,
    pub(crate) message: Box<M>,
}

impl<M> Display for MessageIncoming<M>
where
    M: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "incoming from {}: {}", self.sender, self.message)
    }
}

/// An envelope for an incoming demand, attaching a sender address and responder.
#[derive(DataSize, Debug, Serialize)]
pub struct DemandIncoming<M> {
    /// The sender from which the demand originated.
    pub(crate) sender: NodeId,
    /// The wrapped demand.
    pub(crate) request_msg: Box<M>,
    /// Responder to send the answer down through.
    pub(crate) auto_closing_responder: AutoClosingResponder<Message>,
}

impl<M> Display for DemandIncoming<M>
where
    M: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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

/// A demand for a trie that should be answered.
pub(crate) type TrieDemand = DemandIncoming<TrieRequest>;

/// A demand for consensus protocol data that should be answered.
pub(crate) type ConsensusDemand = DemandIncoming<consensus::ConsensusRequestMessage>;

/// A new message responding to a trie request arrived.
pub(crate) type TrieResponseIncoming = MessageIncoming<TrieResponse>;

/// A new finality signature arrived over the network.
pub(crate) type FinalitySignatureIncoming = MessageIncoming<FinalitySignatureV2>;

/// A request for an object out of storage arrived.
///
/// Note: The variants here are grouped under a common enum, since they are usually handled by the
///       same component. If this changes, split up this type (see `TrieRequestIncoming` for an
///       example).
#[derive(DataSize, Debug, Serialize)]
#[repr(u8)]
pub(crate) enum NetRequest {
    Transaction(Vec<u8>),
    LegacyDeploy(Vec<u8>),
    Block(Vec<u8>),
    BlockHeader(Vec<u8>),
    FinalitySignature(Vec<u8>),
    SyncLeap(Vec<u8>),
    ApprovalsHashes(Vec<u8>),
    BlockExecutionResults(Vec<u8>),
}

impl Display for NetRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetRequest::Transaction(_) => f.write_str("request for transaction"),
            NetRequest::LegacyDeploy(_) => f.write_str("request for legacy deploy"),
            NetRequest::Block(_) => f.write_str("request for block"),
            NetRequest::BlockHeader(_) => f.write_str("request for block header"),
            NetRequest::FinalitySignature(_) => {
                f.write_str("request for gossiped finality signature")
            }
            NetRequest::SyncLeap(_) => f.write_str("request for sync leap"),
            NetRequest::ApprovalsHashes(_) => f.write_str("request for approvals hashes"),
            NetRequest::BlockExecutionResults(_) => {
                f.write_str("request for block execution results")
            }
        }
    }
}

impl NetRequest {
    /// Returns a unique identifier of the requested object.
    pub(crate) fn unique_id(&self) -> Vec<u8> {
        let id = match self {
            NetRequest::Transaction(ref id)
            | NetRequest::LegacyDeploy(ref id)
            | NetRequest::Block(ref id)
            | NetRequest::BlockHeader(ref id)
            | NetRequest::FinalitySignature(ref id)
            | NetRequest::SyncLeap(ref id)
            | NetRequest::ApprovalsHashes(ref id)
            | NetRequest::BlockExecutionResults(ref id) => id,
        };
        let mut unique_id = Vec::with_capacity(id.len() + 1);
        unique_id.push(self.tag() as u8);
        unique_id.extend(id);

        unique_id
    }

    /// Returns the tag associated with the request.
    pub(crate) fn tag(&self) -> Tag {
        match self {
            NetRequest::Transaction(_) => Tag::Transaction,
            NetRequest::LegacyDeploy(_) => Tag::LegacyDeploy,
            NetRequest::Block(_) => Tag::Block,
            NetRequest::BlockHeader(_) => Tag::BlockHeader,
            NetRequest::FinalitySignature(_) => Tag::FinalitySignature,
            NetRequest::SyncLeap(_) => Tag::SyncLeap,
            NetRequest::ApprovalsHashes(_) => Tag::ApprovalsHashes,
            NetRequest::BlockExecutionResults(_) => Tag::BlockExecutionResults,
        }
    }
}

/// A response for a net request.
///
/// See `NetRequest` for notes.
#[derive(Debug, Serialize)]
pub(crate) enum NetResponse {
    Transaction(Arc<[u8]>),
    LegacyDeploy(Arc<[u8]>),
    Block(Arc<[u8]>),
    BlockHeader(Arc<[u8]>),
    FinalitySignature(Arc<[u8]>),
    SyncLeap(Arc<[u8]>),
    ApprovalsHashes(Arc<[u8]>),
    BlockExecutionResults(Arc<[u8]>),
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
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetResponse::Transaction(_) => f.write_str("response, transaction"),
            NetResponse::LegacyDeploy(_) => f.write_str("response, legacy deploy"),
            NetResponse::Block(_) => f.write_str("response, block"),
            NetResponse::BlockHeader(_) => f.write_str("response, block header"),
            NetResponse::FinalitySignature(_) => f.write_str("response, finality signature"),
            NetResponse::SyncLeap(_) => f.write_str("response for sync leap"),
            NetResponse::ApprovalsHashes(_) => f.write_str("response for approvals hashes"),
            NetResponse::BlockExecutionResults(_) => {
                f.write_str("response for block execution results")
            }
        }
    }
}

/// A request for a trie.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct TrieRequest(pub(crate) Vec<u8>);

impl Display for TrieRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "request for trie {}", TrieOrChunkIdDisplay(&self.0))
    }
}

/// A response to a request for a trie.
#[derive(DataSize, Debug, Serialize)]
pub(crate) struct TrieResponse(pub(crate) Vec<u8>);

impl Display for TrieResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("response, trie")
    }
}

#[cfg(test)]
mod tests {
    use super::NetRequest;

    #[test]
    fn unique_id_is_unique_across_variants() {
        let inner_id = b"example".to_vec();

        let a = NetRequest::Transaction(inner_id.clone());
        let b = NetRequest::Block(inner_id);

        assert_ne!(a.unique_id(), b.unique_id());
    }
}
