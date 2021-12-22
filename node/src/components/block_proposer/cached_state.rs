use std::collections::HashMap;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::{deploy_sets::DeployData, BlockProposerDeploySets};
use crate::types::DeployHash;

/// State which is put to storage and loaded on initialization.
#[derive(Serialize, Deserialize, Default, Debug, DataSize)]
pub(crate) struct CachedState {
    pub(super) pending_deploys: HashMap<DeployHash, DeployData>,
    pub(super) pending_transfers: HashMap<DeployHash, DeployData>,
}

impl From<&BlockProposerDeploySets> for CachedState {
    fn from(sets: &BlockProposerDeploySets) -> Self {
        CachedState {
            pending_deploys: sets.pending_deploys.clone(),
            pending_transfers: sets.pending_transfers.clone(),
        }
    }
}
