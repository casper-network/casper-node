//! Errors in constructing and validating indexed Merkle proofs, chunks with indexed Merkle proofs.
use crate::{ChunkWithProof, Digest};

/// Error validating a Merkle proof of a chunk.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum MerkleVerificationError {
    /// Index out of bounds.
    #[error("Index out of bounds. Count: {count}, index: {index}")]
    IndexOutOfBounds {
        /// Count.
        count: u64,
        /// Index.
        index: u64,
    },

    /// Unexpected proof length.
    #[error(
        "Unexpected proof length. Count: {count}, index: {index}, \
         expected proof length: {expected_proof_length}, \
         actual proof length: {actual_proof_length}"
    )]
    UnexpectedProofLength {
        /// Count.
        count: u64,
        /// Index.
        index: u64,
        /// Expected proof length.
        expected_proof_length: u8,
        /// Actual proof length.
        actual_proof_length: usize,
    },
}

/// Error validating a chunk with proof.
#[derive(thiserror::Error, Debug)]
pub enum ChunkWithProofVerificationError {
    /// Indexed Merkle proof verification error.
    #[error(transparent)]
    MerkleVerificationError(#[from] MerkleVerificationError),

    /// Empty Merkle proof for trie with chunk.
    #[error("Chunk with proof has empty merkle proof: {chunk_with_proof:?}")]
    ChunkWithProofHasEmptyMerkleProof {
        /// Chunk with empty Merkle proof.
        chunk_with_proof: ChunkWithProof,
    },

    /// First digest in indexed Merkle proof did not match hash of chunk.
    #[error(
        "First digest in Merkle proof did not match hash of chunk. \
         First digest in indexed Merkle proof: {first_digest_in_indexed_merkle_proof:?}. \
         Hash of chunk: {hash_of_chunk:?}."
    )]
    FirstDigestInMerkleProofDidNotMatchHashOfChunk {
        /// First digest in indexed Merkle proof.
        first_digest_in_indexed_merkle_proof: Digest,
        /// Hash of chunk.
        hash_of_chunk: Digest,
    },
}

/// Error during the construction of a Merkle proof.
#[derive(thiserror::Error, Debug, Eq, PartialEq, Clone)]
pub enum MerkleConstructionError {
    /// Chunk index was out of bounds.
    #[error(
        "Could not construct Merkle proof. Index out of bounds. Count: {count}, index: {index}"
    )]
    IndexOutOfBounds {
        /// Total chunks count.
        count: u64,
        /// Requested index.
        index: u64,
    },
    /// Too many Merkle tree leaves.
    #[error(
        "Could not construct Merkle proof. Too many leaves. Count: {count}, max: {} (u64::MAX)",
        u64::MAX
    )]
    TooManyLeaves {
        /// Total chunks count.
        count: String,
    },
}
