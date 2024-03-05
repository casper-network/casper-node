#[cfg(test)]
mod tests;

use std::{
    cmp::Ord,
    fmt::{Display, Formatter},
};

use datasize::DataSize;
use tracing::debug;

use casper_storage::block_store::types::ApprovalsHashes;
use casper_types::{TransactionHash, TransactionId};

use super::block_acquisition::Acceptance;

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    AcquisitionByIdNotPossible,
    EncounteredNonVacantTransactionState,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::AcquisitionByIdNotPossible => write!(f, "acquisition by id is not possible"),
            Error::EncounteredNonVacantTransactionState => {
                write!(f, "encountered non vacant transaction state")
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(super) enum TransactionIdentifier {
    ByHash(TransactionHash),
    ById(TransactionId),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) enum TransactionAcquisition {
    ByHash(Acquisition<TransactionHash>),
    ById(Acquisition<TransactionId>),
}

impl TransactionAcquisition {
    pub(super) fn new_by_hash(
        transaction_hashes: Vec<TransactionHash>,
        need_execution_result: bool,
    ) -> Self {
        TransactionAcquisition::ByHash(Acquisition::new(transaction_hashes, need_execution_result))
    }

    pub(super) fn apply_transaction(
        &mut self,
        transaction_id: TransactionId,
    ) -> Option<Acceptance> {
        match self {
            TransactionAcquisition::ByHash(acquisition) => {
                acquisition.apply_transaction(transaction_id.transaction_hash())
            }
            TransactionAcquisition::ById(acquisition) => {
                acquisition.apply_transaction(transaction_id)
            }
        }
    }

    pub(super) fn apply_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<(), Error> {
        let new_acquisition = match self {
            TransactionAcquisition::ByHash(acquisition) => {
                let mut new_txn_ids = vec![];
                for ((transaction_hash, txn_state), approvals_hash) in acquisition
                    .inner
                    .drain(..)
                    .zip(approvals_hashes.approvals_hashes())
                {
                    if !matches!(txn_state, TransactionState::Vacant) {
                        return Err(Error::EncounteredNonVacantTransactionState);
                    };
                    let txn_id = match (transaction_hash, approvals_hash) {
                        (TransactionHash::Deploy(deploy_hash), deploy_approvals_hash) => {
                            TransactionId::new(deploy_hash.into(), deploy_approvals_hash)
                        }
                        (TransactionHash::V1(transaction_v1_hash), txn_v1_approvals_hash) => {
                            TransactionId::new(transaction_v1_hash.into(), txn_v1_approvals_hash)
                        }
                    };
                    new_txn_ids.push((txn_id, TransactionState::Vacant));
                }

                TransactionAcquisition::ById(Acquisition {
                    inner: new_txn_ids,
                    need_execution_result: acquisition.need_execution_result,
                })
            }
            TransactionAcquisition::ById(_) => {
                debug!("TransactionAcquisition: attempt to apply approvals hashes on a transaction acquired by ID");
                return Err(Error::AcquisitionByIdNotPossible);
            }
        };

        *self = new_acquisition;
        Ok(())
    }

    pub(super) fn needs_transaction(&self) -> bool {
        match self {
            TransactionAcquisition::ByHash(acq) => acq.needs_transaction().is_some(),
            TransactionAcquisition::ById(acq) => acq.needs_transaction().is_some(),
        }
    }

    pub(super) fn next_needed_transaction(&self) -> Option<TransactionIdentifier> {
        match self {
            TransactionAcquisition::ByHash(acq) => {
                acq.needs_transaction().map(TransactionIdentifier::ByHash)
            }
            TransactionAcquisition::ById(acq) => {
                acq.needs_transaction().map(TransactionIdentifier::ById)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug, Default)]
pub(super) enum TransactionState {
    #[default]
    Vacant,
    HaveTransactionBody,
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct Acquisition<T> {
    inner: Vec<(T, TransactionState)>,
    need_execution_result: bool,
}

impl<T: Copy + Ord> Acquisition<T> {
    fn new(txn_identifiers: Vec<T>, need_execution_result: bool) -> Self {
        let inner = txn_identifiers
            .into_iter()
            .map(|txn_identifier| (txn_identifier, TransactionState::Vacant))
            .collect();
        Acquisition {
            inner,
            need_execution_result,
        }
    }

    fn apply_transaction(&mut self, transaction_identifier: T) -> Option<Acceptance> {
        for item in self.inner.iter_mut() {
            if item.0 == transaction_identifier {
                match item.1 {
                    TransactionState::Vacant => {
                        item.1 = TransactionState::HaveTransactionBody;
                        return Some(Acceptance::NeededIt);
                    }
                    TransactionState::HaveTransactionBody => return Some(Acceptance::HadIt),
                }
            }
        }
        None
    }

    fn needs_transaction(&self) -> Option<T> {
        self.inner
            .iter()
            .find_map(|(txn_identifier, state)| match state {
                TransactionState::Vacant => Some(*txn_identifier),
                TransactionState::HaveTransactionBody => None,
            })
    }
}
