use std::{
    cmp::Ord,
    fmt::{Display, Formatter},
};

use datasize::DataSize;
use tracing::debug;

use super::block_acquisition::Acceptance;
use crate::types::{ApprovalsHashes, DeployHash, DeployId};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    AcquisitionByIdNotPossible,
    EncounteredNonVacantDeployState,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::AcquisitionByIdNotPossible => write!(f, "acquisition by id is not possible"),
            Error::EncounteredNonVacantDeployState => {
                write!(f, "encountered non vacant deploy state")
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) enum DeployIdentifier {
    ByHash(DeployHash),
    ById(DeployId),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) enum DeployAcquisition {
    ByHash(Acquisition<DeployHash>),
    ById(Acquisition<DeployId>),
}

impl DeployAcquisition {
    pub(super) fn new_by_hash(deploy_hashes: Vec<DeployHash>, need_execution_result: bool) -> Self {
        DeployAcquisition::ByHash(Acquisition::new(deploy_hashes, need_execution_result))
    }

    pub(super) fn apply_deploy(&mut self, deploy_id: DeployId) -> Option<Acceptance> {
        match self {
            DeployAcquisition::ByHash(acquisition) => {
                acquisition.apply_deploy(*deploy_id.deploy_hash())
            }
            DeployAcquisition::ById(acquisition) => acquisition.apply_deploy(deploy_id),
        }
    }

    pub(super) fn apply_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<(), Error> {
        let new_acquisition = match self {
            DeployAcquisition::ByHash(acquisition) => {
                let mut new_deploy_ids = vec![];
                for ((deploy_hash, deploy_state), approvals_hash) in acquisition
                    .inner
                    .drain(..)
                    .zip(approvals_hashes.approvals_hashes())
                {
                    if !matches!(deploy_state, DeployState::Vacant) {
                        return Err(Error::EncounteredNonVacantDeployState);
                    };
                    new_deploy_ids.push((
                        DeployId::new(deploy_hash, *approvals_hash),
                        DeployState::Vacant,
                    ));
                }

                DeployAcquisition::ById(Acquisition {
                    inner: new_deploy_ids,
                    need_execution_result: acquisition.need_execution_result,
                })
            }
            DeployAcquisition::ById(_) => {
                debug!("DeployAcquisition: attempt to apply approvals hashes on a deploy acquired by ID");
                return Err(Error::AcquisitionByIdNotPossible);
            }
        };

        *self = new_acquisition;
        Ok(())
    }

    pub(super) fn needs_deploy(&self) -> Option<DeployIdentifier> {
        match self {
            DeployAcquisition::ByHash(acq) => acq.needs_deploy().map(DeployIdentifier::ByHash),
            DeployAcquisition::ById(acq) => acq.needs_deploy().map(DeployIdentifier::ById),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug, Default)]
pub(super) enum DeployState {
    #[default]
    Vacant,
    HaveDeployBody,
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct Acquisition<T> {
    inner: Vec<(T, DeployState)>,
    need_execution_result: bool,
}

impl<T: Copy + Ord> Acquisition<T> {
    fn new(deploy_identifiers: Vec<T>, need_execution_result: bool) -> Self {
        let inner = deploy_identifiers
            .into_iter()
            .map(|deploy_identifier| (deploy_identifier, DeployState::Vacant))
            .collect();
        Acquisition {
            inner,
            need_execution_result,
        }
    }

    fn apply_deploy(&mut self, deploy_identifier: T) -> Option<Acceptance> {
        for item in self.inner.iter_mut() {
            if item.0 == deploy_identifier {
                match item.1 {
                    DeployState::Vacant => {
                        item.1 = DeployState::HaveDeployBody;
                        return Some(Acceptance::NeededIt);
                    }
                    DeployState::HaveDeployBody => return Some(Acceptance::HadIt),
                }
            }
        }
        None
    }

    fn needs_deploy(&self) -> Option<T> {
        self.inner
            .iter()
            .find_map(|(deploy_identifier, state)| match state {
                DeployState::Vacant => Some(*deploy_identifier),
                DeployState::HaveDeployBody => None,
            })
    }
}
