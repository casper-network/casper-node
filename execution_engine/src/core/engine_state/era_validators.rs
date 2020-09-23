use casper_types::{
    auction::{EraId, ValidatorWeights},
    ProtocolVersion,
};

use crate::shared::newtypes::Blake2bHash;

#[derive(Debug)]
pub enum GetEraValidatorsResult {
    /// A successful query operation returned validator weights for given era.
    Success { validator_weights: ValidatorWeights },
    /// Invalid state hash was used to make this request
    RootNotFound,
    /// Wrong era passed. There is no snapshot of validator weights for given era.
    InvalidEra,
    /// Indicates that queried value is missing, or has invalid type.
    ValueError,
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
