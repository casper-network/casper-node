#[cfg(test)]
use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::Approval;
use super::{Deploy, FinalizedApprovals};

/// A deploy combined with a potential set of finalized approvals.
///
/// Represents a deploy, along with a potential set of approvals different from those contained in
/// the deploy itself. If such a set of approvals is present, it indicates that the set contained
/// the deploy was not the set  used to validate the execution of the deploy after consensus.
///
/// A typical case where these can differ is if a deploy is sent with an original set of approvals
/// to the local node, while a second set of approvals makes it to the proposing node. The local
/// node has to adhere to the proposer's approvals to obtain the same outcome.
#[derive(DataSize, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DeployWithFinalizedApprovals {
    /// The deploy that likely has been included in a block.
    deploy: Deploy,
    /// Approvals used to verify the deploy during block execution.
    finalized_approvals: Option<FinalizedApprovals>,
}

impl DeployWithFinalizedApprovals {
    /// Creates a new deploy with finalized approvals from parts.
    pub(crate) fn new(deploy: Deploy, finalized_approvals: Option<FinalizedApprovals>) -> Self {
        Self {
            deploy,
            finalized_approvals,
        }
    }

    /// Creates a naive deploy by potentially substituting the approvals with the finalized
    /// approvals.
    pub(crate) fn into_naive(self) -> Deploy {
        let mut deploy = self.deploy;
        if let Some(finalized_approvals) = self.finalized_approvals {
            deploy.approvals = finalized_approvals.into_inner();
        }

        deploy
    }

    /// Extracts the original deploy by discarding the finalized approvals.
    pub(crate) fn discard_finalized_approvals(self) -> Deploy {
        self.deploy
    }

    #[cfg(test)]
    pub(crate) fn original_approvals(&self) -> &BTreeSet<Approval> {
        self.deploy.approvals()
    }

    #[cfg(test)]
    pub(crate) fn finalized_approvals(&self) -> Option<&FinalizedApprovals> {
        self.finalized_approvals.as_ref()
    }
}
