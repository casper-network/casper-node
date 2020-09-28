use casper_types::{auction::EraId, ProtocolVersion};
use thiserror::Error;

use crate::{core::engine_state::error::Error, shared::newtypes::Blake2bHash};

#[derive(Debug, Error)]
pub enum GetEraValidatorsError {
    /// Invalid state hash was used to make this request
    #[error("Invalid state hash")]
    RootNotFound,
    /// Indicates that queried value is missing, or has invalid type.
    #[error("Value error")]
    ValueError,
    /// Engine state error
    #[error(transparent)]
    Other(#[from] Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetEraValidatorsRequest {
    state_hash: Blake2bHash,
    era_id: EraId,
    protocol_version: ProtocolVersion,
}

impl GetEraValidatorsRequest {
    pub fn new(state_hash: Blake2bHash, era_id: EraId, protocol_version: ProtocolVersion) -> Self {
        GetEraValidatorsRequest {
            state_hash,
            era_id,
            protocol_version,
        }
    }

    pub fn state_hash(&self) -> Blake2bHash {
        self.state_hash
    }

    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}
