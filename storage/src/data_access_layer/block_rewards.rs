use std::collections::BTreeMap;
use thiserror::Error;

use casper_types::{execution::Effects, Digest, ProtocolVersion, PublicKey, U512};

use crate::tracking_copy::TrackingCopyError;

pub struct BlockRewardsRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    rewards: BTreeMap<PublicKey, U512>,
    next_block_height: u64,
    time: u64,
}

impl BlockRewardsRequest {
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        rewards: BTreeMap<PublicKey, U512>,
        next_block_height: u64,
        time: u64,
    ) -> Self {
        BlockRewardsRequest {
            state_hash,
            protocol_version,
            rewards,
            next_block_height,
            time,
        }
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
    pub fn rewards(&self) -> &BTreeMap<PublicKey, U512> {
        &self.rewards
    }

    /// Returns next_block_height.
    pub fn next_block_height(&self) -> u64 {
        self.next_block_height
    }

    /// Returns time.
    pub fn time(&self) -> u64 {
        self.time
    }
}

#[derive(Clone, Error, Debug)]
pub enum BlockRewardsError {
    #[error("Undistributed fees")]
    UndistributedFees,
    #[error("Undistributed rewards")]
    UndistributedRewards,
    #[error("{0}")]
    TrackingCopy(TrackingCopyError),
}

pub enum BlockRewardsResult {
    RootNotFound,
    Failure(BlockRewardsError),
    Success {
        /// State hash after distribution outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the distribution process.
        effects: Effects,
    },
}

impl BlockRewardsResult {
    pub fn is_success(&self) -> bool {
        matches!(self, BlockRewardsResult::Success { .. })
    }
}
