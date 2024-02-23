#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{DeployApproval, TransactionV1Approval};

/// A struct containing a signature of a transaction hash and the public key of the signer.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum TransactionApproval {
    /// A deploy approval.
    Deploy(DeployApproval),
    /// A version 1 transaction approval.
    #[cfg_attr(any(feature = "std", test), serde(rename = "Version1"))]
    V1(TransactionV1Approval),
}

impl From<&DeployApproval> for TransactionApproval {
    fn from(value: &DeployApproval) -> Self {
        Self::Deploy(value.clone())
    }
}

impl From<&TransactionV1Approval> for TransactionApproval {
    fn from(value: &TransactionV1Approval) -> Self {
        Self::V1(value.clone())
    }
}
