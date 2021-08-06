//! Support for querying era validators.
use thiserror::Error;

use datasize::DataSize;

use casper_types::ProtocolVersion;

use crate::{core::engine_state::error::Error, shared::newtypes::Blake2bHash};

/// An enum that represents all possible error conditions of a get era validators request.
#[derive(Debug, Error, DataSize)]
pub enum GetEraValidatorsError {
    /// Invalid state hash was used to make this request
    #[error("Invalid state hash")]
    RootNotFound,
    /// Engine state error
    #[error(transparent)]
    Other(#[from] Error),
    /// EraValidators missing
    #[error("Era validators missing")]
    EraValidatorsMissing,
}

impl GetEraValidatorsError {
    /// Returns `true` if the result represents missing era validators.
    pub fn is_era_validators_missing(&self) -> bool {
        matches!(self, GetEraValidatorsError::EraValidatorsMissing)
    }
}

/// Represents a get era validators request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetEraValidatorsRequest {
    state_hash: Blake2bHash,
    protocol_version: ProtocolVersion,
}

impl GetEraValidatorsRequest {
    /// Creates a new [`GetEraValidatorsRequest`]
    pub fn new(state_hash: Blake2bHash, protocol_version: ProtocolVersion) -> Self {
        GetEraValidatorsRequest {
            state_hash,
            protocol_version,
        }
    }

    /// Returns a state root hash.
    pub fn state_hash(&self) -> Blake2bHash {
        self.state_hash
    }

    /// Returns a protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}
