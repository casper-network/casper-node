use std::collections::BTreeMap;

use datasize::DataSize;

use casper_execution_engine::{
    core::engine_state::GetEraValidatorsRequest, shared::execution_journal::ExecutionJournal,
};
use casper_hashing::Digest;
use casper_types::{EraId, ExecutionResult, ProtocolVersion, PublicKey, U512};

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
#[derive(Debug, DataSize)]
pub struct StepEffectAndUpcomingEraValidators {
    /// Validator sets for all upcoming eras that have already been determined.
    pub upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    /// An [`ExecutionJournal`] created by an era ending.
    pub step_execution_journal: ExecutionJournal,
}

/// A [`Block`] that was the result of execution in the `ContractRuntime` along with any execution
/// effects it may have.
#[derive(Debug, DataSize)]
pub struct BlockAndExecutionEffects {
    /// The [`Block`] the contract runtime executed.
    pub block: Box<Block>,
    /// The results from executing the deploys in the block.
    pub execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
    /// The [`ExecutionJournal`] and the upcoming validator sets determined by the `step`
    pub maybe_step_effect_and_upcoming_era_validators: Option<StepEffectAndUpcomingEraValidators>,
}

impl BlockAndExecutionEffects {
    /// Gets the block.
    pub fn block(&self) -> &Block {
        &self.block
    }
}

impl From<BlockAndExecutionEffects> for Block {
    fn from(block_and_execution_effects: BlockAndExecutionEffects) -> Self {
        *block_and_execution_effects.block
    }
}
