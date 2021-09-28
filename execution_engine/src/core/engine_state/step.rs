//! Support for a step method.
//!
//! A step request executes auction code, slashes validators, evicts validators and distributes
//! rewards.
use std::{collections::BTreeMap, vec::Vec};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr, bytesrepr::ToBytes, CLValueError, EraId, ProtocolVersion, PublicKey,
};

use crate::core::{
    engine_state::{execution_effect::ExecutionEffect, Error},
    execution,
};

/// The definition of a slash item.
#[derive(Debug)]
pub struct SlashItem {
    /// The public key of the validator that will be slashed.
    pub validator_id: PublicKey,
}

impl SlashItem {
    /// Creates a new slash item.
    pub fn new(validator_id: PublicKey) -> Self {
        Self { validator_id }
    }
}

/// The definition of a reward item.
#[derive(Debug)]
pub struct RewardItem {
    /// The public key of the validator that will be rewarded.
    pub validator_id: PublicKey,
    /// Amount of motes that will be distributed as rewards.
    pub value: u64,
}

impl RewardItem {
    /// Creates new reward item.
    pub fn new(validator_id: PublicKey, value: u64) -> Self {
        Self {
            validator_id,
            value,
        }
    }
}

/// The definition of an evict item.
#[derive(Debug)]
pub struct EvictItem {
    /// The public key of the validator that will be evicted.
    pub validator_id: PublicKey,
}

impl EvictItem {
    /// Creates new evict item.
    pub fn new(validator_id: PublicKey) -> Self {
        Self { validator_id }
    }
}

/// Representation of a step request.
#[derive(Debug)]
pub struct StepRequest {
    /// State root hash.
    pub pre_state_hash: Digest,
    /// Protocol version for this request.
    pub protocol_version: ProtocolVersion,
    /// List of validators to be slashed.
    ///
    /// A slashed validator is removed from the next validator set.
    pub slash_items: Vec<SlashItem>,
    /// List of validators that will be rewarded.
    pub reward_items: Vec<RewardItem>,
    /// List of validators to be evicted.
    ///
    /// Compared to a slashing, evictions are deactivating a given validator, but his stake is
    /// unchanged. A further re-activation is possible.
    pub evict_items: Vec<EvictItem>,
    /// If true an auction contract will be executed to compute new era validators.
    pub run_auction: bool,
    /// Specifies which era validators will be returned based on `next_era_id`.
    ///
    /// Intended use is to always specify the current era id + 1 which will return computed era at
    /// the end of this step request.
    pub next_era_id: EraId,
    /// Timestamp in milliseconds representing end of the current era.
    pub era_end_timestamp_millis: u64,
}

impl StepRequest {
    /// Creates new step request.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pre_state_hash: Digest,
        protocol_version: ProtocolVersion,
        slash_items: Vec<SlashItem>,
        reward_items: Vec<RewardItem>,
        evict_items: Vec<EvictItem>,
        run_auction: bool,
        next_era_id: EraId,
        era_end_timestamp_millis: u64,
    ) -> Self {
        Self {
            pre_state_hash,
            protocol_version,
            slash_items,
            reward_items,
            evict_items,
            run_auction,
            next_era_id,
            era_end_timestamp_millis,
        }
    }

    /// Returns list of slashed validators.
    pub fn slashed_validators(&self) -> Result<Vec<PublicKey>, bytesrepr::Error> {
        let mut ret = vec![];
        for slash_item in &self.slash_items {
            let public_key: PublicKey =
                bytesrepr::deserialize(slash_item.validator_id.clone().to_bytes()?)?;
            ret.push(public_key);
        }
        Ok(ret)
    }

    /// Returns all reward factors.
    pub fn reward_factors(&self) -> Result<BTreeMap<PublicKey, u64>, bytesrepr::Error> {
        let mut ret = BTreeMap::new();
        for reward_item in &self.reward_items {
            ret.insert(reward_item.validator_id.clone(), reward_item.value);
        }
        Ok(ret)
    }
}

/// Representation of all possible failures of a step request.
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    /// Invalid state root hash.
    #[error("Root not found: {0:?}")]
    RootNotFound(Digest),
    /// Error creating a tracking copy instance.
    #[error("Tracking copy error: {0}")]
    TrackingCopyError(Error),
    #[error("Get contract error: {0}")]
    /// Error getting a system contract.
    GetContractError(Error),
    /// Error retrieving a system module.
    #[error("Get system module error: {0}")]
    GetSystemModuleError(Error),
    /// Error executing a slashing operation.
    #[error("Slashing error: {0}")]
    SlashingError(Error),
    /// Error executing the auction contract.
    #[error("Auction error: {0}")]
    AuctionError(Error),
    /// Error executing a distribute operation.
    #[error("Distribute error: {0}")]
    DistributeError(Error),
    /// Invalid protocol version.
    #[error("Invalid protocol version: {0}")]
    InvalidProtocolVersion(ProtocolVersion),
    /// Error while (de)serializing data.
    #[error("{0}")]
    BytesRepr(bytesrepr::Error),
    /// Error converting `CLValue`.
    #[error("{0}")]
    CLValueError(CLValueError),
    #[error("Other engine state error: {0}")]
    /// Other engine state error.
    OtherEngineStateError(#[from] Error),
    /// Execution error.
    #[error(transparent)]
    ExecutionError(#[from] execution::Error),
}

impl From<bytesrepr::Error> for StepError {
    fn from(error: bytesrepr::Error) -> Self {
        StepError::BytesRepr(error)
    }
}

impl From<CLValueError> for StepError {
    fn from(error: CLValueError) -> Self {
        StepError::CLValueError(error)
    }
}

/// Represents a successfully executed step request.
#[derive(Debug)]
pub struct StepSuccess {
    /// New state root hash generated after effects were applied.
    pub post_state_hash: Digest,
    /// Effects of executing a step request.
    pub execution_effect: ExecutionEffect,
}
