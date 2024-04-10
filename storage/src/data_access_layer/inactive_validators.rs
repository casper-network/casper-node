use casper_types::{
    execution::Effects, system::auction::Error as AuctionError, BlockTime, Digest, ProtocolVersion,
};
use thiserror::Error;

use crate::{
    system::{runtime_native::Config, transfer::TransferError},
    tracking_copy::TrackingCopyError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InactiveValidatorsUndelegationRequest {
    config: Config,
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    block_time: BlockTime,
}

impl InactiveValidatorsUndelegationRequest {
    pub fn new(
        config: Config,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: BlockTime,
    ) -> Self {
        InactiveValidatorsUndelegationRequest {
            config,
            state_hash,
            protocol_version,
            block_time,
        }
    }

    /// Returns config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns state_hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns protocol_version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns block time.
    pub fn block_time(&self) -> BlockTime {
        self.block_time
    }
}

#[derive(Clone, Error, Debug)]
pub enum InactiveValidatorsUndelegationError {
    #[error("Undistributed rewards")]
    UndistributedRewards,
    #[error(transparent)]
    TrackingCopy(TrackingCopyError),
    #[error("Registry entry not found: {0}")]
    RegistryEntryNotFound(String),
    #[error(transparent)]
    Transfer(TransferError),
    #[error("Auction error: {0}")]
    Auction(AuctionError),
}

#[derive(Debug, Clone)]
pub enum InactiveValidatorsUndelegationResult {
    RootNotFound,
    Failure(InactiveValidatorsUndelegationError),
    Success {
        /// State hash after distribution outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the distribution process.
        effects: Effects,
    },
}

impl InactiveValidatorsUndelegationResult {
    pub fn is_success(&self) -> bool {
        matches!(self, InactiveValidatorsUndelegationResult::Success { .. })
    }
}
