use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use casper_types::{
    execution::{ExecutionResult, ExecutionResultV1},
    BlockHash,
};

/// Version 1 metadata related to a single deploy prior to `casper-node` v2.0.0.
#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub(crate) struct DeployMetadataV1 {
    /// The hash of the single block containing the related deploy, along with the results of
    /// executing it.
    ///
    /// Due to reasons, this was implemented as a map, despite the guarantee that there will only
    /// ever be a single entry.
    pub(super) execution_results: HashMap<BlockHash, ExecutionResultV1>,
}

impl From<DeployMetadataV1> for ExecutionResult {
    fn from(v1_results: DeployMetadataV1) -> Self {
        let v1_result = v1_results
            .execution_results
            .into_iter()
            .next()
            // Safe to unwrap as it's guaranteed to contain exactly one entry.
            .expect("must be exactly one result")
            .1;
        ExecutionResult::V1(v1_result)
    }
}
