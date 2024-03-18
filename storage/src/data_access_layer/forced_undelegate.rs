use casper_types::{
    execution::Effects, system::auction::Error as AuctionError, Digest, ProtocolVersion,
};
use thiserror::Error;

use crate::{
    system::{runtime_native::Config, transfer::TransferError},
    tracking_copy::TrackingCopyError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedUndelegateRequest {
    config: Config,
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    block_time: u64,
}

impl ForcedUndelegateRequest {
    pub fn new(
        config: Config,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: u64,
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
    pub fn block_time(&self) -> u64 {
        self.block_time
    }
}

#[derive(Clone, Error, Debug)]
pub enum ForcedUndelegateError {
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
pub enum ForcedUndelegateResult {
    RootNotFound,
    Failure(ForcedUndelegateError),
    Success {
        /// State hash after distribution outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the distribution process.
        effects: Effects,
    },
}

impl ForcedUndelegateResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}
