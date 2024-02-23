use std::collections::BTreeSet;
use thiserror::Error;

use crate::system::{runtime_native::TransferConfig, transfer::TransferError};
use casper_types::{
    account::AccountHash, execution::Effects, Digest, FeeHandling, ProtocolVersion, TransferAddr,
};

use crate::tracking_copy::TrackingCopyError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeRequest {
    config: TransferConfig,
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    block_time: u64,
    fee_handling: FeeHandling,
    administrative_accounts: BTreeSet<AccountHash>,
}

impl FeeRequest {
    pub fn new(
        config: TransferConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        administrative_accounts: BTreeSet<AccountHash>,
        fee_handling: FeeHandling,
        block_time: u64,
    ) -> Self {
        FeeRequest {
            config,
            state_hash,
            protocol_version,
            administrative_accounts,
            fee_handling,
            block_time,
        }
    }

    /// Returns config.
    pub fn config(&self) -> &TransferConfig {
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

    /// Returns fee handling setting.
    pub fn fee_handling(&self) -> FeeHandling {
        self.fee_handling
    }

    /// Returns block time.
    pub fn block_time(&self) -> u64 {
        self.block_time
    }

    /// Returns administrative accounts.
    pub fn administrative_accounts(&self) -> &BTreeSet<AccountHash> {
        &self.administrative_accounts
    }
}

#[derive(Clone, Error, Debug)]
pub enum FeeError {
    #[error("Undistributed fees")]
    NoFeesDistributed,
    #[error(transparent)]
    TrackingCopy(TrackingCopyError),
    #[error("Registry entry not found: {0}")]
    RegistryEntryNotFound(String),
    #[error(transparent)]
    Transfer(TransferError),
    #[error("Named keys not found")]
    NamedKeysNotFound,
}

#[derive(Debug, Clone)]
pub enum FeeResult {
    RootNotFound,
    Failure(FeeError),
    Success {
        /// List of transfers that happened during execution.
        transfers: Vec<TransferAddr>,
        /// State hash after fee distribution outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the fee distribution process.
        effects: Effects,
    },
}

impl FeeResult {
    pub fn is_success(&self) -> bool {
        matches!(self, FeeResult::Success { .. })
    }
}
