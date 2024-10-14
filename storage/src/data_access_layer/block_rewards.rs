use std::collections::BTreeMap;

use thiserror::Error;

use casper_types::{
    execution::Effects, system::auction::Error as AuctionError, BlockTime, Digest, ProtocolVersion,
    PublicKey, U512,
};

use crate::{
    system::{runtime_native::Config, transfer::TransferError},
    tracking_copy::TrackingCopyError,
};

/// Block rewards request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRewardsRequest {
    config: Config,
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    rewards: BTreeMap<PublicKey, Vec<U512>>,
    block_time: BlockTime,
}

impl BlockRewardsRequest {
    /// Ctor.
    pub fn new(
        config: Config,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: BlockTime,
        rewards: BTreeMap<PublicKey, Vec<U512>>,
    ) -> Self {
        BlockRewardsRequest {
            config,
            state_hash,
            protocol_version,
            rewards,
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

    /// Returns rewards.
    pub fn rewards(&self) -> &BTreeMap<PublicKey, Vec<U512>> {
        &self.rewards
    }

    /// Returns block time.
    pub fn block_time(&self) -> BlockTime {
        self.block_time
    }
}

/// Block rewards error.
#[derive(Clone, Error, Debug)]
pub enum BlockRewardsError {
    /// Undistributed rewards error.
    #[error("Undistributed rewards")]
    UndistributedRewards,
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

/// Block reward result.
#[derive(Debug, Clone)]
pub enum BlockRewardsResult {
    /// Root not found in global state.
    RootNotFound,
    /// Block rewards failure error.
    Failure(BlockRewardsError),
    /// Success result.
    Success {
        /// State hash after distribution outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the distribution process.
        effects: Effects,
    },
}

impl BlockRewardsResult {
    /// Returns true if successful, else false.
    pub fn is_success(&self) -> bool {
        matches!(self, BlockRewardsResult::Success { .. })
    }
}
