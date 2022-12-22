use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::Gas;

use super::DeployHeader;

/// Information about how much block limit a deploy will consume.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
pub(crate) struct Footprint {
    pub(crate) header: DeployHeader,
    pub(crate) gas_estimate: Gas,
    pub(crate) size_estimate: usize,
    pub(crate) is_transfer: bool,
}
