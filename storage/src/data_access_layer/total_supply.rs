use crate::tracking_copy::TrackingCopyError;
use casper_types::{Digest, ProtocolVersion, U512};

/// Request for total supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TotalSupplyRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
    enable_addressable_entity: bool,
}

impl TotalSupplyRequest {
    /// Creates an instance of TotalSupplyRequest.
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        enable_addressable_entity: bool,
    ) -> Self {
        TotalSupplyRequest {
            state_hash,
            protocol_version,
            enable_addressable_entity,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Enable the addressable entity and migrate accounts/contracts to entities.
    pub fn enable_addressable_entity(&self) -> bool {
        self.enable_addressable_entity
    }
}

/// Represents a result of a `total_supply` request.
#[derive(Debug)]
pub enum TotalSupplyResult {
    /// Invalid state root hash.
    RootNotFound,
    /// The mint is not found.
    MintNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// The total supply at the specified state hash.
    Success {
        /// The total supply in motes.
        total_supply: U512,
    },
    /// Failed to get total supply.
    Failure(TrackingCopyError),
}
