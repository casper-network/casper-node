use casper_types::{Key, StoredValue, URef, U512};
use hashing::Digest;

use crate::storage::trie::merkle_proof::TrieMerkleProof;

#[derive(Debug)]
pub enum BalanceResult {
    RootNotFound,
    Success {
        motes: U512,
        proof: Box<TrieMerkleProof<Key, StoredValue>>,
    },
}

impl BalanceResult {
    pub fn motes(&self) -> Option<&U512> {
        match self {
            BalanceResult::Success { motes, .. } => Some(motes),
            _ => None,
        }
    }

    pub fn proof(self) -> Option<TrieMerkleProof<Key, StoredValue>> {
        match self {
            BalanceResult::Success { proof, .. } => Some(*proof),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRequest {
    state_hash: Digest,
    purse_uref: URef,
}

impl BalanceRequest {
    pub fn new(state_hash: Digest, purse_uref: URef) -> Self {
        BalanceRequest {
            state_hash,
            purse_uref,
        }
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    pub fn purse_uref(&self) -> URef {
        self.purse_uref
    }
}
