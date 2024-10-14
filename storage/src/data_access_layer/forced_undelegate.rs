use casper_types::{
    execution::Effects, system::auction::Error as AuctionError, BlockTime, Digest, ProtocolVersion,
};
use thiserror::Error;

use crate::{
    system::{runtime_native::Config, transfer::TransferError},
    tracking_copy::TrackingCopyError,
};

/// Forced undelegate request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedUndelegateRequest {
    config: Config,
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    block_time: BlockTime,
}

impl ForcedUndelegateRequest {
    /// Ctor.
    pub fn new(
        config: Config,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: BlockTime,
    ) -> Self {
        Self {
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

/// Forced undelegation error.
#[derive(Clone, Error, Debug)]
pub enum ForcedUndelegateError {
    /// Tracking copy error.
    #[error(transparent)]
    TrackingCopy(TrackingCopyError),
    /// Registry entry not found error.
    #[error("Registry entry not found: {0}")]
    RegistryEntryNotFound(String),
    /// Transfer error.
    #[error(transparent)]
    Transfer(TransferError),
    /// Auction error.
    #[error("Auction error: {0}")]
    Auction(AuctionError),
}

/// Forced undelegation result.
#[derive(Debug, Clone)]
pub enum ForcedUndelegateResult {
    /// Root hash not found in global state.
    RootNotFound,
    /// Forced undelegation failed.
    Failure(ForcedUndelegateError),
    /// Forced undelgation succeeded.
    Success {
        /// State hash after distribution outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the distribution process.
        effects: Effects,
    },
}

impl ForcedUndelegateResult {
    /// Returns true if successful, else false.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}
