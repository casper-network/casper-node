#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[allow(unused)]
pub enum MerkleVerificationError {
    #[error("Index out of bounds. Count: {count}, index: {index}")]
    IndexOutOfBounds { count: u64, index: u64 },
    #[error(
        "Unexpected proof length. Count: {count}, index: {index}, \
         expected proof length: {expected_proof_length}, \
         actual proof length: {actual_proof_length}"
    )]
    UnexpectedProofLength {
        count: u64,
        index: u64,
        expected_proof_length: u8,
        actual_proof_length: usize,
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
