//! Announcements of incoming network messages.
//!
//! Any event suffixed -`Incoming` is usually the arrival of a specific network message.

use crate::{
    components::{consensus, gossiper, small_network::GossipedAddress},
    types::{Deploy, FinalitySignature},
};

/// A new consensus message arrived.
pub(crate) struct ConsensusMessageIncoming(pub(crate) consensus::ConsensusMessage);

/// A new deploy gossiper message arrived.
pub(crate) struct DeployGossiperIncoming(pub(crate) gossiper::Message<Deploy>);

/// A new address gossiper message arrived.
pub(crate) struct AddressGossiperIncoming(pub(crate) gossiper::Message<GossipedAddress>);

/// A new request for a object out of storage arrived.
///
/// Note: The variants here are grouped under a common enum, since they are usually handled by the
///       same component. If this changes, split up this type (see `TrieRequestIncoming` for an
///       example).
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

/// A new request for a trie arrived.
///
/// See `NetRequestIncoming` for notes.
pub(crate) struct TrieRequestIncoming(pub(crate) Vec<u8>);

/// A response for a net request arrived.
///
/// See `NetRequestIncoming` for notes.
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

/// A response for a trie request arrived.
///
/// See `NetRequestIncoming` for notes.
pub(crate) struct TrieResponseIncoming(pub(crate) Vec<u8>);

/// A new finality signature arrived over the network.
pub(crate) struct FinalitySignatureIncoming(pub(crate) Box<FinalitySignature>);
