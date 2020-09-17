use casper_types::{Key, U512};

use crate::{
    shared::newtypes::Blake2bHash,
};

#[derive(Debug)]
pub enum BalanceResult {
    RootNotFound,
    ValueNotFound(String),
    CircularReference(String),
    Success(U512),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRequest {
    state_hash: Blake2bHash,
    purse_key: Key,
}

impl BalanceRequest {
    pub fn new(state_hash: Blake2bHash, purse_key: Key) -> Self {
        BalanceRequest {
            state_hash,
            purse_key,
        }
    }

    pub fn state_hash(&self) -> Blake2bHash {
        self.state_hash
    }

    pub fn purse_key(&self) -> Key {
        self.purse_key
    }
}