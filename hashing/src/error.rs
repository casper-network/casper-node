#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum MerkleVerificationError {
    #[cfg(test)]
    #[error("Index out of bounds. Count: {count}, index: {index}")]
    IndexOutOfBounds { count: u64, index: u64 },
    #[cfg(test)]
    #[error(
        "Unexpected proof length. Count: {count}, index: {index}, \
         expected proof length: {expected_proof_length}, \
         actual proof length: {actual_proof_length}"
    )]
    UnexpectedProofLength {
        count: u64,
        index: u64,
        expected_proof_length: u64,
        actual_proof_length: usize,
    },
}

#[derive(thiserror::Error, Debug, Eq, PartialEq)]
pub enum MerkleConstructionError {
    #[cfg(test)]
    #[error("Could not construct Merkle proof. Empty proof must have index 0. Index: {index}")]
    EmptyProofMustHaveIndex { index: u64 },
    #[error(
        "Could not construct Merkle proof. Index out of bounds.  Count: {count}, index: {index}"
    )]
    #[cfg(test)]
    IndexOutOfBounds { count: u64, index: u64 },
    #[error("The chunk has incorrect proof")]
    IncorrectChunkProof,
    #[error("The idexed merkle proof is incorrect")]
    IncorrectIndexedMerkleProof,
}
