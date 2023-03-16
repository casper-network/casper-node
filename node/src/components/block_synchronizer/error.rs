use std::fmt::{Display, Formatter};

use datasize::DataSize;
use derive_more::From;

use casper_hashing::Digest;

use super::deploy_acquisition;

use crate::types::{BlockHash, DeployId};

#[derive(Clone, Copy, From, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum BlockAcquisitionError {
    InvalidStateTransition,
    BlockHashMismatch {
        expected: BlockHash,
        actual: BlockHash,
    },
    RootHashMismatch {
        expected: Digest,
        actual: Digest,
    },
    InvalidAttemptToAcquireExecutionResults,
    #[from]
    InvalidAttemptToApplyApprovalsHashes(deploy_acquisition::Error),
    InvalidAttemptToApplyDeploy {
        deploy_id: DeployId,
    },
    InvalidAttemptToMarkComplete,
    InvalidAttemptToEnqueueBlockForExecution,
    ExecutionResults(super::execution_results_acquisition::Error),
    GlobalStateAcquisition(super::global_state_acquisition::Error),
    EraValidatorsAcquisition(super::era_validators_acquisition::Error),
    DuplicateGlobalStateAcquisition(Digest),
    BlockHeaderMissing,
    MissingEraValidatorWeights,
}

impl Display for BlockAcquisitionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockAcquisitionError::InvalidStateTransition => write!(f, "invalid state transition"),
            BlockAcquisitionError::InvalidAttemptToMarkComplete => {
                write!(f, "invalid attempt to mark complete")
            }
            BlockAcquisitionError::InvalidAttemptToAcquireExecutionResults => {
                write!(
                    f,
                    "invalid attempt to acquire execution results while in a terminal state"
                )
            }
            BlockAcquisitionError::BlockHashMismatch { expected, actual } => {
                write!(
                    f,
                    "block hash mismatch: expected {} actual: {}",
                    expected, actual
                )
            }
            BlockAcquisitionError::RootHashMismatch { expected, actual } => write!(
                f,
                "root hash mismatch: expected {} actual: {}",
                expected, actual
            ),
            BlockAcquisitionError::ExecutionResults(error) => {
                write!(f, "execution results error: {}", error)
            }
            BlockAcquisitionError::InvalidAttemptToApplyApprovalsHashes(error) => write!(
                f,
                "invalid attempt to apply approvals hashes results: {}",
                error
            ),
            BlockAcquisitionError::InvalidAttemptToEnqueueBlockForExecution => {
                write!(f, "invalid attempt to enqueue block for execution")
            }
            BlockAcquisitionError::InvalidAttemptToApplyDeploy { deploy_id } => {
                write!(f, "invalid attempt to apply deploy: {}", deploy_id)
            }
            BlockAcquisitionError::GlobalStateAcquisition(error) => {
                write!(f, "error when acquiring global state: {}", error)
            }
            BlockAcquisitionError::EraValidatorsAcquisition(error) => {
                write!(f, "error when acquiring era validators: {}", error)
            }
            BlockAcquisitionError::DuplicateGlobalStateAcquisition(state_root_hash) => {
                write!(
                    f,
                    "found duplicate global state acquisition for state root hash: {}",
                    state_root_hash
                )
            }
            BlockAcquisitionError::BlockHeaderMissing => {
                write!(
                    f,
                    "failed to get block header from storage even if it was expected to exist"
                )
            }
            BlockAcquisitionError::MissingEraValidatorWeights => {
                write!(f, "missing era validator weights")
            }
        }
    }
}
