use std::fmt::{Display, Formatter};

use datasize::DataSize;
use derive_more::From;

use casper_types::{Digest, TransactionHash, TransactionId};

use super::deploy_acquisition;

use casper_types::BlockHash;

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
    InvalidAttemptToApplyTransaction {
        txn_id: TransactionId,
    },
    MissingApprovalsHashes(TransactionHash),
    InvalidAttemptToMarkComplete,
    InvalidAttemptToEnqueueBlockForExecution,
    ExecutionResults(super::execution_results_acquisition::Error),
    InvalidTransactionType,
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
            BlockAcquisitionError::InvalidAttemptToApplyTransaction { txn_id } => {
                write!(f, "invalid attempt to apply transaction: {}", txn_id)
            }
            BlockAcquisitionError::MissingApprovalsHashes(missing_txn_hash) => {
                write!(
                    f,
                    "missing approvals hashes for transaction {}",
                    missing_txn_hash
                )
            }
            BlockAcquisitionError::InvalidTransactionType => {
                write!(f, "invalid transaction identifier",)
            }
        }
    }
}
