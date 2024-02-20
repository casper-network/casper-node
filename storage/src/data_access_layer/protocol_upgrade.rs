use casper_types::{execution::Effects, Digest, ProtocolUpgradeConfig};

use crate::system::protocol_upgrade::ProtocolUpgradeError;

/// Request to upgrade the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolUpgradeRequest {
    config: ProtocolUpgradeConfig,
}

impl ProtocolUpgradeRequest {
    /// Creates a new instance of ProtocolUpgradeRequest.
    pub fn new(config: ProtocolUpgradeConfig) -> Self {
        ProtocolUpgradeRequest { config }
    }

    /// Get the protocol upgrade config.
    pub fn config(&self) -> &ProtocolUpgradeConfig {
        &self.config
    }

    /// Get the pre_state_hash to apply protocol upgrade to.
    pub fn pre_state_hash(&self) -> Digest {
        self.config.pre_state_hash()
    }
}

/// Response to attempt to upgrade the protocol.
#[derive(Debug, Clone)]
pub enum ProtocolUpgradeResult {
    /// Global state root not found.
    RootNotFound,
    /// Protocol upgraded successfully.
    Success {
        /// State hash after protocol upgrade is committed to the global state.
        post_state_hash: Digest,
        /// Effects of protocol upgrade.
        effects: Effects,
    },
    /// Failed to upgrade protocol.
    Failure(ProtocolUpgradeError),
}

impl ProtocolUpgradeResult {
    /// Is success.
    pub fn is_success(&self) -> bool {
        matches!(self, ProtocolUpgradeResult::Success { .. })
    }

    /// Is an error
    pub fn is_err(&self) -> bool {
        match self {
            ProtocolUpgradeResult::RootNotFound | ProtocolUpgradeResult::Failure(_) => true,
            ProtocolUpgradeResult::Success { .. } => false,
        }
    }
}

impl From<ProtocolUpgradeError> for ProtocolUpgradeResult {
    fn from(err: ProtocolUpgradeError) -> Self {
        ProtocolUpgradeResult::Failure(err)
    }
}
