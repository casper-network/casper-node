use casper_types::{URef, U512};

use crate::shared::newtypes::Blake2bHash;

#[derive(Debug)]
pub enum BalanceResult {
    RootNotFound,
    Success(U512),
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
