//! Types for balance queries.
use casper_types::{global_state::TrieMerkleProof, Digest, Key, StoredValue, URef, U512};

use crate::tracking_copy::TrackingCopyError;

/// Result enum that represents all possible outcomes of a balance request.
#[derive(Debug)]
pub enum BalanceResult {
    /// Returned if a passed state root hash is not found.
    RootNotFound,
    /// A query returned a balance.
    Success {
        /// Purse balance.
        motes: U512,
        /// A proof that the given value is present in the Merkle trie.
        proof: Box<TrieMerkleProof<Key, StoredValue>>,
    },
    Failure(TrackingCopyError),
}

impl BalanceResult {
    /// Returns the amount of motes for a [`BalanceResult::Success`] variant.
    pub fn motes(&self) -> Option<&U512> {
        match self {
            BalanceResult::Success { motes, .. } => Some(motes),
            _ => None,
        }
    }

    /// Returns the Merkle proof for a given [`BalanceResult::Success`] variant.
    pub fn proof(self) -> Option<TrieMerkleProof<Key, StoredValue>> {
        match self {
            BalanceResult::Success { proof, .. } => Some(*proof),
            _ => None,
        }
    }
}

/// Represents a balance request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRequest {
    state_hash: Digest,
    purse_uref: URef,
}

impl BalanceRequest {
    /// Creates a new [`BalanceRequest`].
    pub fn new(state_hash: Digest, purse_uref: URef) -> Self {
        BalanceRequest {
            state_hash,
            purse_uref,
        }
    }

    /// Returns a state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns a purse [`URef`].
    pub fn purse_uref(&self) -> URef {
        self.purse_uref
    }
}
