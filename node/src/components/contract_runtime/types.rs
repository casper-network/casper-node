use std::{collections::BTreeMap, sync::Arc};

use datasize::DataSize;

use casper_execution_engine::{
    core::engine_state::GetEraValidatorsRequest, shared::execution_journal::ExecutionJournal,
};
use casper_hashing::Digest;
use casper_types::{EraId, ExecutionResult, ProtocolVersion, PublicKey, U512};

use crate::types::{ApprovalsHashes, Block, DeployHash, DeployHeader};

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
#[derive(Clone, Debug, DataSize)]
pub(crate) struct StepEffectAndUpcomingEraValidators {
    /// Validator sets for all upcoming eras that have already been determined.
    pub(crate) upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    /// An [`ExecutionJournal`] created by an era ending.
    pub(crate) step_execution_journal: ExecutionJournal,
}

#[doc(hidden)]
/// A [`Block`] that was the result of execution in the `ContractRuntime` along with any execution
/// effects it may have.
#[derive(Clone, Debug, DataSize)]
pub struct BlockAndExecutionResults {
    /// The [`Block`] the contract runtime executed.
    pub(crate) block: Arc<Block>,
    /// The [`ApprovalsHashes`] for the deploys in this block.
    pub(crate) approvals_hashes: Box<ApprovalsHashes>,
    /// The results from executing the deploys in the block.
    pub(crate) execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
    /// The [`ExecutionJournal`] and the upcoming validator sets determined by the `step`
    pub(crate) maybe_step_effect_and_upcoming_era_validators:
        Option<StepEffectAndUpcomingEraValidators>,
}
