use std::vec::Vec;

use casper_types::{ProtocolVersion, U512};

use crate::shared::newtypes::Blake2bHash;

#[derive(Debug)]
pub struct SlashItem {
    pub validator_id: Vec<u8>,
    pub value: U512,
}

impl SlashItem {
    pub fn new(validator_id: Vec<u8>, value: U512) -> Self {
        Self {
            validator_id,
            value,
        }
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
}
