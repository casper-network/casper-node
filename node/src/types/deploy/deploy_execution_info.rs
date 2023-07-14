use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{execution::ExecutionResult, BlockHash};

/// The block hash and height in which a given deploy was executed, along with the execution result
/// if known.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeployExecutionInfo {
    pub(crate) block_hash: BlockHash,
    pub(crate) block_height: u64,
    pub(crate) execution_result: Option<ExecutionResult>,
}
