use std::collections::BTreeSet;
use thiserror::Error;

use crate::system::{
    runtime_native::{Config as NativeRuntimeConfig, TransferConfig},
    transfer::TransferError,
};
use casper_types::{
    account::AccountHash, execution::Effects, Digest, FeeHandling, ProtocolVersion, TransferAddr,
};

use crate::tracking_copy::TrackingCopyError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeRequest {
    config: NativeRuntimeConfig,
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    block_time: u64,
}

impl FeeRequest {
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        block_time: u64,
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
    pub fn block_time(&self) -> u64 {
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
    #[error("Administrative accounts not found")]
    AdministrativeAccountsNotFound,
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
