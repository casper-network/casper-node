use std::{collections::BTreeMap, sync::Arc};

use datasize::DataSize;
use serde::Serialize;

use casper_storage::data_access_layer::EraValidatorsRequest;
use casper_types::{
    contract_messages::Messages,
    execution::{Effects, ExecutionResult},
    BlockHash, BlockHeaderV2, BlockV2, DeployHash, DeployHeader, Digest, EraId, ProtocolVersion,
    PublicKey, Timestamp, U512,
};

use crate::types::ApprovalsHashes;

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

impl From<ValidatorWeightsByEraIdRequest> for EraValidatorsRequest {
    fn from(input: ValidatorWeightsByEraIdRequest) -> Self {
        EraValidatorsRequest::new(input.state_hash, input.protocol_version)
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

#[derive(DataSize, Debug, Clone, Serialize)]
/// Wrapper for speculative execution prestate.
pub struct SpeculativeExecutionState {
    /// State root on top of which to execute deploy.
    pub state_root_hash: Digest,
    /// Block time.
    pub block_time: Timestamp,
    /// Protocol version used when creating the original block.
    pub protocol_version: ProtocolVersion,
}

/// State to use to construct the next block in the blockchain. Includes the state root hash for the
/// execution engine as well as certain values the next header will be based on.
#[derive(DataSize, Default, Debug, Clone, Serialize)]
pub struct ExecutionPreState {
    /// The height of the next `Block` to be constructed. Note that this must match the height of
    /// the `FinalizedBlock` used to generate the block.
    next_block_height: u64,
    /// The state root to use when executing deploys.
    pre_state_root_hash: Digest,
    /// The parent hash of the next `Block`.
    parent_hash: BlockHash,
    /// The accumulated seed for the pseudo-random number generator to be incorporated into the
    /// next `Block`, where additional entropy will be introduced.
    parent_seed: Digest,
}

impl ExecutionPreState {
    pub(crate) fn new(
        next_block_height: u64,
        pre_state_root_hash: Digest,
        parent_hash: BlockHash,
        parent_seed: Digest,
    ) -> Self {
        ExecutionPreState {
            next_block_height,
            pre_state_root_hash,
            parent_hash,
            parent_seed,
        }
    }

    /// Creates instance of `ExecutionPreState` from given block header nad Merkle tree hash
    /// activation point.
    pub fn from_block_header(block_header: &BlockHeaderV2) -> Self {
        ExecutionPreState {
            pre_state_root_hash: *block_header.state_root_hash(),
            next_block_height: block_header.height() + 1,
            parent_hash: block_header.block_hash(),
            parent_seed: *block_header.accumulated_seed(),
        }
    }

    // The height of the next `Block` to be constructed. Note that this must match the height of
    /// the `FinalizedBlock` used to generate the block.
    pub fn next_block_height(&self) -> u64 {
        self.next_block_height
    }
    /// The state root to use when executing deploys.
    pub fn pre_state_root_hash(&self) -> Digest {
        self.pre_state_root_hash
    }
    /// The parent hash of the next `Block`.
    pub fn parent_hash(&self) -> BlockHash {
        self.parent_hash
    }
    /// The accumulated seed for the pseudo-random number generator to be incorporated into the
    /// next `Block`, where additional entropy will be introduced.
    pub fn parent_seed(&self) -> Digest {
        self.parent_seed
    }
}
