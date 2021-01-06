use std::convert::{TryFrom, TryInto};

use casper_execution_engine::core::engine_state::era_validators::GetEraValidatorsRequest;
use casper_types::{auction::ValidatorWeights, bytesrepr::ToBytes};

use crate::engine_server::{ipc, mappings::MappingError};

impl TryFrom<ipc::GetEraValidatorsRequest> for GetEraValidatorsRequest {
    type Error = MappingError;

    fn try_from(
        mut pb_get_era_validators_request: ipc::GetEraValidatorsRequest,
    ) -> Result<Self, Self::Error> {
        let pre_state_hash = pb_get_era_validators_request
            .get_parent_state_hash()
            .try_into()
            .map_err(|_| MappingError::InvalidStateHash("parent_state_hash".to_string()))?;

        let protocol_version = pb_get_era_validators_request.take_protocol_version().into();

        Ok(GetEraValidatorsRequest::new(
            pre_state_hash,
            protocol_version,
        ))
    }
}

impl TryFrom<ValidatorWeights> for ipc::ValidatorWeights {
    type Error = MappingError;

    fn try_from(validator_weights: ValidatorWeights) -> Result<Self, Self::Error> {
        let mut pb_validator_weights = ipc::ValidatorWeights::new();

        for (public_key, weight) in validator_weights {
            let mut pb_validator_weight = ipc::ValidatorWeights_ValidatorWeight::new();
            pb_validator_weight.set_public_key_bytes(public_key.to_bytes()?);
            pb_validator_weight.set_weight(weight.into());

            pb_validator_weights
                .mut_validator_weights()
                .push(pb_validator_weight);
        }

        Ok(pb_validator_weights)
    }
}
