use std::collections::BTreeMap;

use datasize::DataSize;

use crate::types::DeployHash;

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug, Default)]
pub(crate) enum DeployState {
    #[default]
    Vacant,
    HaveDeployBody,
    HaveDeployBodyWithEffects,
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(crate) struct DeployAcquisition {
    inner: BTreeMap<DeployHash, DeployState>,
    need_execution_result: bool,
}

impl DeployAcquisition {
    pub(super) fn new(deploy_hashes: Vec<DeployHash>, need_execution_result: bool) -> Self {
        let inner = deploy_hashes
            .into_iter()
            .map(|dh| (dh, DeployState::Vacant))
            .collect();
        DeployAcquisition {
            inner,
            need_execution_result,
        }
    }

    pub(crate) fn apply_deploy(&mut self, deploy_hash: DeployHash) {
        self.inner.insert(deploy_hash, DeployState::HaveDeployBody);
    }

    pub(crate) fn apply_execution_effect(&mut self, deploy_hash: DeployHash) {
        self.inner
            .insert(deploy_hash, DeployState::HaveDeployBodyWithEffects);
    }

    pub(crate) fn needs_deploy(&self) -> Option<DeployHash> {
        self.inner.iter().find_map(|(k, v)| match v {
            DeployState::Vacant => Some(*k),
            DeployState::HaveDeployBody | DeployState::HaveDeployBodyWithEffects => None,
        })
    }
}
