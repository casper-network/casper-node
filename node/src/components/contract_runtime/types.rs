use std::{collections::BTreeMap, sync::Arc};

use datasize::DataSize;

use casper_storage::data_access_layer::EraValidatorsRequest;
use casper_types::{
    contract_messages::Messages,
    execution::{Effects, ExecutionResult},
    BlockV2, DeployHash, DeployHeader, Digest, EraId, ProtocolVersion, PublicKey, U512,
};
use serde::Serialize;

use crate::types::ApprovalsHashes;

/// Request for validator weights for a specific era.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorWeightsByEraIdRequest {
    state_hash: Digest,
    era_id: EraId,
    protocol_version: ProtocolVersion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TotalSupplyRequest {
    pub state_hash: Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundSeigniorageRateRequest {
    pub state_hash: Digest,
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

impl From<ValidatorWeightsByEraIdRequest> for EraValidatorsRequest {
    fn from(input: ValidatorWeightsByEraIdRequest) -> Self {
        EraValidatorsRequest::new(input.state_hash, input.protocol_version)
    }
}

impl TotalSupplyRequest {
    pub fn new(state_hash: Digest) -> Self {
        TotalSupplyRequest { state_hash }
    }
}

impl RoundSeigniorageRateRequest {
    pub fn new(state_hash: Digest) -> Self {
        RoundSeigniorageRateRequest { state_hash }
    }
}

/// Effects from running step and the next era validators that are gathered when an era ends.
#[derive(Clone, Debug, DataSize)]
pub(crate) struct StepEffectsAndUpcomingEraValidators {
    /// Validator sets for all upcoming eras that have already been determined.
    pub(crate) upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    /// An [`Effects`] created by an era ending.
    pub(crate) step_effects: Effects,
}

#[derive(Clone, Debug, DataSize, PartialEq, Eq, Serialize)]
pub(crate) struct ExecutionArtifact {
    pub(crate) deploy_hash: DeployHash,
    pub(crate) deploy_header: DeployHeader,
    pub(crate) execution_result: ExecutionResult,
    pub(crate) messages: Messages,
}

impl ExecutionArtifact {
    pub(crate) fn new(
        deploy_hash: DeployHash,
        deploy_header: DeployHeader,
        execution_result: ExecutionResult,
        messages: Messages,
    ) -> Self {
        Self {
            deploy_hash,
            deploy_header,
            execution_result,
            messages,
        }
    }
}

#[doc(hidden)]
/// A [`Block`] that was the result of execution in the `ContractRuntime` along with any execution
/// effects it may have.
#[derive(Clone, Debug, DataSize)]
pub struct BlockAndExecutionResults {
    /// The [`Block`] the contract runtime executed.
    pub(crate) block: Arc<BlockV2>,
    /// The [`ApprovalsHashes`] for the deploys in this block.
    pub(crate) approvals_hashes: Box<ApprovalsHashes>,
    /// The results from executing the deploys in the block.
    pub(crate) execution_results: Vec<ExecutionArtifact>,
    /// The [`Effects`] and the upcoming validator sets determined by the `step`
    pub(crate) maybe_step_effects_and_upcoming_era_validators:
        Option<StepEffectsAndUpcomingEraValidators>,
}
