use std::collections::BTreeSet;
use thiserror::Error;

use crate::system::{
    runtime_native::{Config as NativeRuntimeConfig, TransferConfig},
    transfer::TransferError,
};
use casper_types::{
    account::AccountHash, execution::Effects, BlockTime, Digest, FeeHandling, ProtocolVersion,
    Transfer,
};

use crate::tracking_copy::TrackingCopyError;

/// Fee request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeRequest {
    config: NativeRuntimeConfig,
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    block_time: BlockTime,
}

impl FeeRequest {
    /// Ctor.
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: BlockTime,
    ) -> Self {
        FeeRequest {
            config,
            state_hash,
            protocol_version,
            block_time,
        }
    }

    /// Returns config.
    pub fn config(&self) -> &NativeRuntimeConfig {
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
    pub fn fee_handling(&self) -> &FeeHandling {
        self.config.fee_handling()
    }

    /// Returns block time.
    pub fn block_time(&self) -> BlockTime {
        self.block_time
    }

    /// Returns administrative accounts, if any.
    pub fn administrative_accounts(&self) -> Option<&BTreeSet<AccountHash>> {
        match self.config.transfer_config() {
            TransferConfig::Administered {
                administrative_accounts,
                ..
            } => Some(administrative_accounts),
            TransferConfig::Unadministered => None,
        }
    }

    /// Should we attempt to distribute fees?
    pub fn should_distribute_fees(&self) -> bool {
        // we only distribute if chainspec FeeHandling == Accumulate
        // and if there are administrative accounts to receive the fees.
        // the various public networks do not use this option.
        if !self.fee_handling().is_accumulate() {
            return false;
        }

        matches!(
            self.config.transfer_config(),
            TransferConfig::Administered { .. }
        )
    }
}

/// Fee error.
#[derive(Clone, Error, Debug)]
pub enum FeeError {
    /// No fees distributed error.
    #[error("Undistributed fees")]
    NoFeesDistributed,
    /// Tracking copy error.
    #[error(transparent)]
    TrackingCopy(TrackingCopyError),
    /// Registry entry not found.
    #[error("Registry entry not found: {0}")]
    RegistryEntryNotFound(String),
    /// Transfer error.
    #[error(transparent)]
    Transfer(TransferError),
    /// Named keys not found.
    #[error("Named keys not found")]
    NamedKeysNotFound,
    /// Administrative accounts not found.
    #[error("Administrative accounts not found")]
    AdministrativeAccountsNotFound,
}

/// Fee result.
#[derive(Debug, Clone)]
pub enum FeeResult {
    /// Root not found in global state.
    RootNotFound,
    /// Failure result.
    Failure(FeeError),
    /// Success result.
    Success {
        /// List of transfers that happened during execution.
        transfers: Vec<Transfer>,
        /// State hash after fee distribution outcome is committed to the global state.
        post_state_hash: Digest,
        /// Effects of the fee distribution process.
        effects: Effects,
    },
}

impl FeeResult {
    /// Returns true if successful, else false.
    pub fn is_success(&self) -> bool {
        matches!(self, FeeResult::Success { .. })
    }
}
