use std::{collections::BTreeMap, fmt::Display, vec::Vec};

use core::fmt;
use uint::static_assertions::_core::fmt::Formatter;

use casper_types::{
    auction::EraId, bytesrepr, bytesrepr::ToBytes, CLValueError, Key, ProtocolVersion, PublicKey,
    U512,
};

use crate::{
    core::engine_state::{Error, GetEraValidatorsError},
    shared::{newtypes::Blake2bHash, TypeMismatch},
};

#[derive(Debug)]
pub struct SlashItem {
    pub validator_id: PublicKey,
}

impl SlashItem {
    pub fn new(validator_id: PublicKey) -> Self {
        Self { validator_id }
    }
}

#[derive(Debug)]
pub struct RewardItem {
    pub validator_id: PublicKey,
    pub value: u64,
}

impl RewardItem {
    pub fn new(validator_id: PublicKey, value: u64) -> Self {
        Self {
            validator_id,
            value,
        }
    }
}

#[derive(Debug)]
pub struct StepRequest {
    pub pre_state_hash: Blake2bHash,
    pub protocol_version: ProtocolVersion,

    pub slash_items: Vec<SlashItem>,
    pub reward_items: Vec<RewardItem>,
    pub run_auction: bool,
    pub next_era_id: EraId,
}

impl StepRequest {
    pub fn new(
        pre_state_hash: Blake2bHash,
        protocol_version: ProtocolVersion,
        slash_items: Vec<SlashItem>,
        reward_items: Vec<RewardItem>,
        run_auction: bool,
        next_era_id: EraId,
    ) -> Self {
        Self {
            pre_state_hash,
            protocol_version,
            slash_items,
            reward_items,
            run_auction,
            next_era_id,
        }
    }

    pub fn slashed_validators(&self) -> Result<Vec<PublicKey>, bytesrepr::Error> {
        let mut ret = vec![];
        for slash_item in &self.slash_items {
            let public_key: PublicKey =
                bytesrepr::deserialize(slash_item.validator_id.clone().to_bytes()?)?;
            ret.push(public_key);
        }
        Ok(ret)
    }

    pub fn reward_factors(&self) -> Result<BTreeMap<PublicKey, u64>, bytesrepr::Error> {
        let mut ret = BTreeMap::new();
        for reward_item in &self.reward_items {
            ret.insert(reward_item.validator_id, reward_item.value);
        }
        Ok(ret)
    }
}

#[derive(Debug)]
pub enum StepResult {
    RootNotFound,
    PreconditionError,
    SlashingError(Error),
    AuctionError(Error),
    DistributeError(Error),
    InvalidProtocolVersion,
    KeyNotFound(Key),
    TypeMismatch(TypeMismatch),
    Serialization(bytesrepr::Error),
    CLValueError(CLValueError),
    GetEraValidatorsError(GetEraValidatorsError),
    EraValidatorsMissing(EraId),
    Success {
        post_state_hash: Blake2bHash,
        next_era_validators: BTreeMap<PublicKey, U512>,
    },
}

impl Display for StepResult {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
