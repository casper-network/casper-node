use std::{
    convert::Infallible,
    fmt::{Debug, Display},
    hash::Hash,
};

use derive_more::Display;
use serde::{de::DeserializeOwned, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use thiserror::Error;

use casper_execution_engine::storage::trie::{TrieOrChunk, TrieOrChunkId};
use casper_hashing::{error::ChunkWithProofVerificationError, Digest};
use casper_types::EraId;

use crate::types::{BlockHash, BlockHeader};

/// An identifier for a specific type implementing the `Item` trait.  Each different implementing
/// type should have a unique `Tag` variant.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize_repr,
    Deserialize_repr,
    Debug,
    Display,
)]
#[repr(u8)]
pub enum Tag {
    /// A deploy.
    Deploy,
    /// Finalized approvals for a deploy.
    FinalizedApprovals,
    /// A block.
    Block,
    /// A gossiped public listening address.
    GossipedAddress,
    /// A block requested by its height in the linear chain.
    BlockAndMetadataByHeight,
    /// A block header requested by its hash.
    BlockHeaderByHash,
    /// A block header and its finality signatures requested by its height in the linear chain.
    BlockHeaderAndFinalitySignaturesByHeight,
    /// A trie or chunk from the global Merkle tree in the execution engine.
    TrieOrChunk,
}

/// A trait which allows an implementing type to be used by the gossiper and fetcher components, and
/// furthermore allows generic network messages to include this type due to the provision of the
/// type-identifying `TAG`.
pub(crate) trait Item:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display + Eq
{
    /// The type of ID of the item.
    type Id: Copy + Eq + Hash + Serialize + DeserializeOwned + Send + Sync + Debug + Display;
    /// The error type returned when validating to get the ID of the item.
    type ValidationError: std::error::Error + Debug;
    /// The tag representing the type of the item.
    const TAG: Tag;
    /// Whether the item's ID _is_ the complete item or not.
    const ID_IS_COMPLETE_ITEM: bool;

    /// Checks cryptographic validity of the item, and returns an error if invalid.
    fn validate(
        &self,
        verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError>;

    /// The ID of the specific item.
    fn id(&self, verifiable_chunked_hash_activation: EraId) -> Self::Id;
}

/// Error type simply conveying that chunk validation failed.
#[derive(Debug, Error)]
#[error("Chunk validation failed")]
pub(crate) struct ChunkValidationError;

impl Item for TrieOrChunk {
    type Id = TrieOrChunkId;
    type ValidationError = ChunkWithProofVerificationError;
    const TAG: Tag = Tag::TrieOrChunk;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(
        &self,
        _verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        match self {
            TrieOrChunk::Trie(_) => Ok(()),
            TrieOrChunk::ChunkWithProof(chunk_with_proof) => chunk_with_proof.verify(),
        }
    }

    fn id(&self, _verifiable_chunked_hash_activation: EraId) -> Self::Id {
        match self {
            TrieOrChunk::Trie(node_bytes) => TrieOrChunkId(0, Digest::hash(&node_bytes)),
            TrieOrChunk::ChunkWithProof(chunked_data) => TrieOrChunkId(
                chunked_data.proof().index(),
                chunked_data.proof().root_hash(),
            ),
        }
    }
}

impl Item for BlockHeader {
    type Id = BlockHash;
    type ValidationError = Infallible;
    const TAG: Tag = Tag::BlockHeaderByHash;
    const ID_IS_COMPLETE_ITEM: bool = false;

    fn validate(
        &self,
        _verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        Ok(())
    }

    fn id(&self, verifiable_chunked_hash_activation: EraId) -> Self::Id {
        self.hash(verifiable_chunked_hash_activation)
    }
}
