#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{execution::ExecutionResult, BlockHash};

/// The block hash and height in which a given deploy was executed, along with the execution result
/// if known.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExecutionInfo {
    /// The hash of the block in which the deploy was executed.
    pub block_hash: BlockHash,
    /// The height of the block in which the deploy was executed.
    pub block_height: u64,
    /// The execution result if known.
    pub execution_result: Option<ExecutionResult>,
}
