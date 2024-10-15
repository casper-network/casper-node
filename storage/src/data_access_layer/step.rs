//! Support for a step method.
//!
//! A step request executes auction code, slashes validators, evicts validators and distributes
//! rewards.

use std::vec::Vec;
use thiserror::Error;

use casper_types::{execution::Effects, CLValueError, Digest, EraId, ProtocolVersion, PublicKey};

use crate::{
    global_state::error::Error as GlobalStateError,
    system::runtime_native::{Config, TransferConfig},
    tracking_copy::TrackingCopyError,
};

/// The definition of a slash item.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
    /// Config
    config: Config,

    /// State root hash.
    state_hash: Digest,

    /// Protocol version for this request.
    protocol_version: ProtocolVersion,
    /// List of validators to be slashed.
    ///
    /// A slashed validator is removed from the next validator set.
    slash_items: Vec<SlashItem>,
    /// List of validators to be evicted.
    ///
    /// Compared to a slashing, evictions are deactivating a given validator, but his stake is
    /// unchanged. A further re-activation is possible.
    evict_items: Vec<EvictItem>,
    /// Specifies which era validators will be returned based on `next_era_id`.
    ///
    /// Intended use is to always specify the current era id + 1 which will return computed era at
    /// the end of this step request.
    next_era_id: EraId,

    /// Timestamp in milliseconds representing end of the current era.
    era_end_timestamp_millis: u64,
}

impl StepRequest {
    /// Creates new step request.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Config,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        slash_items: Vec<SlashItem>,
        evict_items: Vec<EvictItem>,
        next_era_id: EraId,
        era_end_timestamp_millis: u64,
    ) -> Self {
        Self {
            config,
            state_hash,
            protocol_version,
            slash_items,
            evict_items,
            next_era_id,
            era_end_timestamp_millis,
        }
    }

    /// Returns the config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the transfer config.
    pub fn transfer_config(&self) -> TransferConfig {
        self.config.transfer_config().clone()
    }

    /// Returns list of slashed validators.
    pub fn slashed_validators(&self) -> Vec<PublicKey> {
        self.slash_items
            .iter()
            .map(|si| si.validator_id.clone())
            .collect()
    }

    /// Returns pre_state_hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns protocol_version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns slash_items.
    pub fn slash_items(&self) -> &Vec<SlashItem> {
        &self.slash_items
    }

    /// Returns evict_items.
    pub fn evict_items(&self) -> &Vec<EvictItem> {
        &self.evict_items
    }
    /// Returns next_era_id.
    pub fn next_era_id(&self) -> EraId {
        self.next_era_id
    }

    /// Returns era_end_timestamp_millis.
    pub fn era_end_timestamp_millis(&self) -> u64 {
        self.era_end_timestamp_millis
    }
}

/// Representation of all possible failures of a step request.
#[derive(Clone, Error, Debug)]
pub enum StepError {
    /// Error using the auction contract.
    #[error("Auction error")]
    Auction,
    /// Error executing a slashing operation.
    #[error("Slashing error")]
    SlashingError,
    /// Tracking copy error.
    #[error("{0}")]
    TrackingCopy(TrackingCopyError),
    /// Failed to find auction contract.
    #[error("Auction not found")]
    AuctionNotFound,
    /// Failed to find mint contract.
    #[error("Mint not found")]
    MintNotFound,
}

impl From<TrackingCopyError> for StepError {
    fn from(tce: TrackingCopyError) -> Self {
        Self::TrackingCopy(tce)
    }
}

impl From<GlobalStateError> for StepError {
    fn from(gse: GlobalStateError) -> Self {
        Self::TrackingCopy(TrackingCopyError::Storage(gse))
    }
}

impl From<CLValueError> for StepError {
    fn from(cve: CLValueError) -> Self {
        StepError::TrackingCopy(TrackingCopyError::CLValue(cve))
    }
}

/// Outcome of running step process.
#[derive(Debug)]
pub enum StepResult {
    /// Global state root not found.
    RootNotFound,
    /// Step process ran successfully.
    Success {
        /// State hash after step outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the step process.
        effects: Effects,
    },
    /// Failed to upgrade protocol.
    Failure(StepError),
}

impl StepResult {
    /// Returns if step is successful.
    pub fn is_success(&self) -> bool {
        matches!(self, StepResult::Success { .. })
    }
}
