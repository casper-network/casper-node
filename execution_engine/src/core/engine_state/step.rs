use std::vec::Vec;

use casper_types::{bytesrepr, Key, ProtocolVersion, PublicKey, U512};

use crate::shared::{newtypes::Blake2bHash, TypeMismatch};
use casper_types::bytesrepr::FromBytes;
use core::fmt;
use std::fmt::Display;
use uint::static_assertions::_core::fmt::Formatter;

#[derive(Debug)]
pub struct SlashItem {
    pub validator_id: Vec<u8>,
}

impl SlashItem {
    pub fn new(validator_id: Vec<u8>) -> Self {
        Self { validator_id }
    }
}

#[derive(Debug)]
pub struct RewardItem {
    pub validator_id: Vec<u8>,
    pub value: U512,
}

impl RewardItem {
    pub fn new(validator_id: Vec<u8>, value: U512) -> Self {
        Self {
            validator_id,
            value,
        }
    }
}

#[derive(Debug)]
pub struct StepRequest {
    pub parent_state_hash: Blake2bHash,
    pub protocol_version: ProtocolVersion,

    pub slash_items: Vec<SlashItem>,
    pub reward_items: Vec<RewardItem>,
}

impl StepRequest {
    pub fn new(
        parent_state_hash: Blake2bHash,
        protocol_version: ProtocolVersion,
        slash_items: Vec<SlashItem>,
        reward_items: Vec<RewardItem>,
    ) -> Self {
        Self {
            parent_state_hash,
            protocol_version,
            slash_items,
            reward_items,
        }
    }

    pub fn slashed_validators(&self) -> Result<Vec<PublicKey>, bytesrepr::Error> {
        let mut ret = vec![];
        for slash_item in &self.slash_items {
            let (public_key, _) = PublicKey::from_bytes(slash_item.validator_id.as_slice())?;
            ret.push(public_key);
        }
        Ok(ret)
    }
}

#[derive(Debug)]
pub enum StepResult {
    RootNotFound,
    PreconditionError,
    InvalidProtocolVersion,
    KeyNotFound(Key),
    TypeMismatch(TypeMismatch),
    Serialization(bytesrepr::Error),
    Success { post_state_hash: Blake2bHash },
}

impl Display for StepResult {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
