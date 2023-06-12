//! Errors in constructing and validating indexed Merkle proofs, chunks with indexed Merkle proofs.

use alloc::string::String;
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

use super::{ChunkWithProof, Digest};
use crate::bytesrepr;

/// Possible hashing errors.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The digest length was an incorrect size.
    IncorrectDigestLength(usize),
    /// There was a decoding error.
    Base16DecodeError(base16::DecodeError),
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Error::IncorrectDigestLength(length) => {
                write!(
                    formatter,
                    "incorrect digest length {}, expected length {}.",
                    length,
                    Digest::LENGTH
                )
            }
            Error::Base16DecodeError(error) => {
                write!(formatter, "base16 decode error: {}", error)
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::IncorrectDigestLength(_) => None,
            Error::Base16DecodeError(error) => Some(error),
        }
    }
}

/// Error validating a Merkle proof of a chunk.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MerkleVerificationError {
    /// Index out of bounds.
    IndexOutOfBounds {
        /// Count.
        count: u64,
        /// Index.
        index: u64,
    },

    /// Unexpected proof length.
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

impl Display for MerkleVerificationError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            MerkleVerificationError::IndexOutOfBounds { count, index } => {
                write!(
                    formatter,
                    "index out of bounds - count: {}, index: {}",
                    count, index
                )
            }
            MerkleVerificationError::UnexpectedProofLength {
                count,
                index,
                expected_proof_length,
                actual_proof_length,
            } => {
                write!(
                    formatter,
                    "unexpected proof length - count: {}, index: {}, expected length: {}, actual \
                    length: {}",
                    count, index, expected_proof_length, actual_proof_length
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for MerkleVerificationError {}

/// Error validating a chunk with proof.
#[derive(Debug)]
#[non_exhaustive]
pub enum ChunkWithProofVerificationError {
    /// Indexed Merkle proof verification error.
    MerkleVerificationError(MerkleVerificationError),

    /// Empty Merkle proof for trie with chunk.
    ChunkWithProofHasEmptyMerkleProof {
        /// Chunk with empty Merkle proof.
        chunk_with_proof: ChunkWithProof,
    },
    /// Unexpected Merkle root hash.
    UnexpectedRootHash,
    /// Bytesrepr error.
    Bytesrepr(bytesrepr::Error),

    /// First digest in indexed Merkle proof did not match hash of chunk.
    FirstDigestInMerkleProofDidNotMatchHashOfChunk {
        /// First digest in indexed Merkle proof.
        first_digest_in_indexed_merkle_proof: Digest,
        /// Hash of chunk.
        hash_of_chunk: Digest,
    },
}

impl Display for ChunkWithProofVerificationError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            ChunkWithProofVerificationError::MerkleVerificationError(error) => {
                write!(formatter, "{}", error)
            }
            ChunkWithProofVerificationError::ChunkWithProofHasEmptyMerkleProof {
                chunk_with_proof,
            } => {
                write!(
                    formatter,
                    "chunk with proof has empty merkle proof: {:?}",
                    chunk_with_proof
                )
            }
            ChunkWithProofVerificationError::UnexpectedRootHash => {
                write!(formatter, "merkle proof has an unexpected root hash")
            }
            ChunkWithProofVerificationError::Bytesrepr(error) => {
                write!(
                    formatter,
                    "bytesrepr error computing chunkable hash: {}",
                    error
                )
            }
            ChunkWithProofVerificationError::FirstDigestInMerkleProofDidNotMatchHashOfChunk {
                first_digest_in_indexed_merkle_proof,
                hash_of_chunk,
            } => {
                write!(
                    formatter,
                    "first digest in merkle proof did not match hash of chunk - first digest: \
                    {:?}, hash of chunk: {:?}",
                    first_digest_in_indexed_merkle_proof, hash_of_chunk
                )
            }
        }
    }
}

impl From<MerkleVerificationError> for ChunkWithProofVerificationError {
    fn from(error: MerkleVerificationError) -> Self {
        ChunkWithProofVerificationError::MerkleVerificationError(error)
    }
}

#[cfg(feature = "std")]
impl StdError for ChunkWithProofVerificationError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            ChunkWithProofVerificationError::MerkleVerificationError(error) => Some(error),
            ChunkWithProofVerificationError::Bytesrepr(error) => Some(error),
            ChunkWithProofVerificationError::ChunkWithProofHasEmptyMerkleProof { .. }
            | ChunkWithProofVerificationError::UnexpectedRootHash
            | ChunkWithProofVerificationError::FirstDigestInMerkleProofDidNotMatchHashOfChunk {
                ..
            } => None,
        }
    }
}

/// Error during the construction of a Merkle proof.
#[derive(Debug, Eq, PartialEq, Clone)]
#[non_exhaustive]
pub enum MerkleConstructionError {
    /// Chunk index was out of bounds.
    IndexOutOfBounds {
        /// Total chunks count.
        count: u64,
        /// Requested index.
        index: u64,
    },
    /// Too many Merkle tree leaves.
    TooManyLeaves {
        /// Total chunks count.
        count: String,
    },
}

impl Display for MerkleConstructionError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            MerkleConstructionError::IndexOutOfBounds { count, index } => {
                write!(
                    formatter,
                    "could not construct merkle proof - index out of bounds - count: {}, index: {}",
                    count, index
                )
            }
            MerkleConstructionError::TooManyLeaves { count } => {
                write!(
                    formatter,
                    "could not construct merkle proof - too many leaves - count: {}, max: {} \
                    (u64::MAX)",
                    count,
                    u64::MAX
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for MerkleConstructionError {}
