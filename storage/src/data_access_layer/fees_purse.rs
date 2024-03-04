use crate::tracking_copy::TrackingCopyError;
use casper_types::{account::AccountHash, Digest, ProtocolVersion, URef};

/// Fees purse handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeesPurseHandling {
    ToProposer(AccountHash),
    Accumulate,
    Burn,
}

/// Request for reward purse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeesPurseRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    fees_purse_handling: FeesPurseHandling,
}

impl FeesPurseRequest {
    /// New instance of request.
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        fees_purse_handling: FeesPurseHandling,
    ) -> Self {
        FeesPurseRequest {
            state_hash,
            protocol_version,
            fees_purse_handling,
        }
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// New instance of request.
    pub fn to_proposer(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        proposer: AccountHash,
    ) -> Self {
        FeesPurseRequest {
            state_hash,
            protocol_version,
            fees_purse_handling: FeesPurseHandling::ToProposer(proposer),
        }
    }

    pub fn fees_purse_handling(&self) -> &FeesPurseHandling {
        &self.fees_purse_handling
    }
}

/// Result of reward purse request.
#[derive(Debug)]
pub enum FeesPurseResult {
    /// Invalid state root hash.
    RootNotFound,
    /// The system contract registry was not found. This is a valid outcome
    /// on older networks, which did not have the system contract registry prior
    /// to protocol version 1.4
    SystemContractRegistryNotFound,
    /// The named entity was not found in the registry.
    NamedEntityNotFound(String),
    /// Successful request.
    Success {
        /// Reward purse.
        purse: URef,
    },
    /// Failed to get requested data.
    Failure(TrackingCopyError),
}
