use crate::{
    contract_runtime::{ExecutionArtifact, StepOutcome},
    types::DataSize,
};
use casper_storage::block_store::types::ApprovalsHashes;
use casper_types::BlockV2;
use std::sync::Arc;

#[doc(hidden)]
/// A [`Block`] that was the result of execution in the `ContractRuntime` along with any execution
/// effects it may have.
#[derive(Clone, Debug, DataSize)]
pub struct BlockAndExecutionArtifacts {
    /// The [`Block`] the contract runtime executed.
    pub(crate) block: Arc<BlockV2>,
    /// The [`ApprovalsHashes`] for the transactions in this block.
    pub(crate) approvals_hashes: Box<ApprovalsHashes>,
    /// The results from executing the transactions in the block.
    pub(crate) execution_artifacts: Vec<ExecutionArtifact>,
    /// The [`Effects`] and the upcoming validator sets determined by the `step`
    pub(crate) step_outcome: Option<StepOutcome>,
}
