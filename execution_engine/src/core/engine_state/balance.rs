use casper_types::{Key, URef, U512};

use crate::{
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::merkle_proof::TrieMerkleProof,
};

#[derive(Debug)]
pub enum BalanceResult {
    RootNotFound,
    Success {
        motes: U512,
        purse_proof: Box<TrieMerkleProof<Key, StoredValue>>,
        balance_proof: Box<TrieMerkleProof<Key, StoredValue>>,
    },
}

impl BalanceResult {
    pub fn motes(&self) -> Option<&U512> {
        match self {
            BalanceResult::Success { motes, .. } => Some(motes),
            _ => None,
        }
    }

    pub fn proofs(
        self,
    ) -> Option<(
        TrieMerkleProof<Key, StoredValue>,
        TrieMerkleProof<Key, StoredValue>,
    )> {
        match self {
            BalanceResult::Success {
                purse_proof: main_purse_proof,
                balance_proof,
                ..
            } => Some((*main_purse_proof, *balance_proof)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRequest {
    state_hash: Blake2bHash,
    purse_uref: URef,
}

impl BalanceRequest {
    pub fn new(state_hash: Blake2bHash, purse_uref: URef) -> Self {
        BalanceRequest {
            state_hash,
            purse_uref,
        }
    }

    pub fn state_hash(&self) -> Blake2bHash {
        self.state_hash
    }

    pub fn purse_uref(&self) -> URef {
        self.purse_uref
    }
}
