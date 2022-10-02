use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::{Approval, Deploy, DeployHash};

/// The hash of a deploy (or transfer) together with signatures approving it for execution.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct DeployHashWithApprovals {
    /// The hash of the deploy.
    deploy_hash: DeployHash,
    /// The approvals for the deploy.
    approvals: BTreeSet<Approval>,
}

impl DeployHashWithApprovals {
    /// Creates a new `DeployWithApprovals`.
    pub(crate) fn new(deploy_hash: DeployHash, approvals: BTreeSet<Approval>) -> Self {
        Self {
            deploy_hash,
            approvals,
        }
    }

    /// Returns the deploy hash.
    pub(crate) fn deploy_hash(&self) -> &DeployHash {
        &self.deploy_hash
    }

    /// Returns the approvals.
    pub(crate) fn approvals(&self) -> &BTreeSet<Approval> {
        &self.approvals
    }
}

impl From<&Deploy> for DeployHashWithApprovals {
    fn from(deploy: &Deploy) -> Self {
        DeployHashWithApprovals {
            deploy_hash: *deploy.id(),
            approvals: deploy.approvals().clone(),
        }
    }
}
