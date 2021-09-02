use std::collections::{BTreeMap, HashMap};

use casper_execution_engine::core::engine_state::{
    execution_effect::ExecutionEffect, GetEraValidatorsRequest,
};
use casper_types::{EraId, ExecutionResult, ProtocolVersion, PublicKey, U512};
use hashing::Digest;

use crate::types::{Block, DeployHash, DeployHeader};

/// Request for validator weights for a specific era.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorWeightsByEraIdRequest {
    state_hash: Digest,
    era_id: EraId,
    protocol_version: ProtocolVersion,
}

impl ValidatorWeightsByEraIdRequest {
    /// Constructs a new ValidatorWeightsByEraIdRequest.
    pub fn new(state_hash: Digest, era_id: EraId, protocol_version: ProtocolVersion) -> Self {
        ValidatorWeightsByEraIdRequest {
            state_hash,
            era_id,
            protocol_version,
        }
    }

    /// Get the state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Get the era id.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Get the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}

impl From<ValidatorWeightsByEraIdRequest> for GetEraValidatorsRequest {
    fn from(input: ValidatorWeightsByEraIdRequest) -> Self {
        GetEraValidatorsRequest::new(input.state_hash, input.protocol_version)
    }
}

/// Request for era validators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EraValidatorsRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
}

impl EraValidatorsRequest {
    /// Constructs a new EraValidatorsRequest.
    pub fn new(state_hash: Digest, protocol_version: ProtocolVersion) -> Self {
        EraValidatorsRequest {
            state_hash,
            protocol_version,
        }
    }

    /// Get the state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Get the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}

impl From<EraValidatorsRequest> for GetEraValidatorsRequest {
    fn from(input: EraValidatorsRequest) -> Self {
        GetEraValidatorsRequest::new(input.state_hash, input.protocol_version)
    }
}

/// Effects from running step and the next era validators that are gathered when an era ends.
#[derive(Debug)]
pub struct StepEffectAndUpcomingEraValidators {
    /// An [`ExecutionEffect`] created by an era ending.
    pub step_execution_effect: ExecutionEffect,
    /// Validator sets for all upcoming eras that have already been determined.
    pub upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
}

/// A [`Block`] that was the result of execution in the `ContractRuntime` along with any execution
/// effects it may have.
#[derive(Debug)]
pub struct BlockAndExecutionEffects {
    /// The [`Block`] the contract runtime executed.
    pub block: Block,
    /// The results from executing the deploys in the block.
    pub execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
    /// The [`ExecutionEffect`] and the upcoming validator sets determined by the `step`
    pub maybe_step_effect_and_upcoming_era_validators: Option<StepEffectAndUpcomingEraValidators>,
}
