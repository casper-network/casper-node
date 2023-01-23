use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::Approval;

/// A set of approvals that has been agreed upon by consensus to approve of a specific deploy.
#[derive(DataSize, Debug, Deserialize, Eq, PartialEq, Serialize, Clone)]
pub(crate) struct FinalizedApprovals(BTreeSet<Approval>);

impl FinalizedApprovals {
    pub(crate) fn new(approvals: BTreeSet<Approval>) -> Self {
        Self(approvals)
    }

    pub(crate) fn inner(&self) -> &BTreeSet<Approval> {
        &self.0
    }

    pub(crate) fn into_inner(self) -> BTreeSet<Approval> {
        self.0
    }
}
