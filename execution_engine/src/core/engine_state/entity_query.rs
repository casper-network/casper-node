//! Support for global state queries specifically for retrieveing entities by AccountHash.
use casper_storage::global_state::storage::trie::merkle_proof::TrieMerkleProof;
use casper_types::contracts::AccountHash;
use casper_types::{AddressableEntity, Digest, Key, StoredValue};

#[derive(Debug)]
pub enum EntityResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// Successful query.
    Success(AddressableEntity),
}

/// Request for an Entity query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRequest {
    state_hash: Digest,
    account_hash: AccountHash,
}

impl EntityRequest {
    pub fn new(state_hash: Digest, account_hash: AccountHash) -> Self {
        Self {
            state_hash,
            account_hash,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns a key.
    pub fn key(&self) -> AccountHash {
        self.account_hash
    }
}
