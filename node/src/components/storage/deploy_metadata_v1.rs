use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use casper_types::{execution::ExecutionResultV1, BlockHash};

/// Version 1 metadata related to a single deploy prior to `casper-node` v2.0.0.
#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub(super) struct DeployMetadataV1 {
    /// The block hashes of blocks containing the related deploy, along with the results of
    /// executing the related deploy in the context of one or more blocks.
    pub(super) execution_results: HashMap<BlockHash, ExecutionResultV1>,
}
