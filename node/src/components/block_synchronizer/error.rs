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
    ExecutionResults(super::execution_results_acquisition::Error),
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
            BlockAcquisitionError::InvalidAttemptToApplyDeploy { deploy_id } => {
                write!(f, "invalid attempt to apply deploy: {}", deploy_id)
            }
        }
    }
}
