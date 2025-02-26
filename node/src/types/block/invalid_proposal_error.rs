use crate::types::DataSize;
use casper_types::{FinalitySignatureId, InvalidTransaction, TransactionHash};

#[derive(DataSize, Debug, Clone)]
pub(crate) enum InvalidProposalError {
    Appendable(String),
    InvalidTransaction(String),
    AncestorTransactionReplay {
        replayed_transaction_hash: TransactionHash,
    },
    UnfetchedTransaction {
        transaction_hash: TransactionHash,
    },
    RewardSignaturesMissingCitedBlock {
        cited_block_height: u64,
    },
    RewardSignatureReplay {
        cited_block_height: u64,
    },
    InvalidFinalitySignature(FinalitySignatureId),
    ExceedsLaneLimit {
        lane_id: u8,
    },
    UnsupportedLane,
    InvalidGasPrice {
        proposed_gas_price: u8,
        current_gas_price: u8,
    },
    InvalidApprovalsHash(String),
    CompetingApprovals {
        transaction_hash: TransactionHash,
    },
    UnableToFetch,
    FailedFetcherValidation,
    UnexpectedFetchStatus,
    FetchedIncorrectTransactionById {
        expected_transaction_hash: TransactionHash,
        actual_transaction_hash: TransactionHash,
    },
    TransactionFetchingAborted,
    FetcherError(String),
    FinalitySignatureFetchingAborted,
    TransactionReplayPreviousEra {
        transaction_era_id: u64,
        proposed_block_era_id: u64,
    },
}

impl From<crate::types::appendable_block::AddError> for Box<InvalidProposalError> {
    fn from(appendable_block_error: crate::types::appendable_block::AddError) -> Self {
        Box::new(InvalidProposalError::Appendable(format!(
            "{}",
            appendable_block_error
        )))
    }
}

impl From<InvalidTransaction> for Box<InvalidProposalError> {
    fn from(invalid_transaction: InvalidTransaction) -> Self {
        Box::new(InvalidProposalError::InvalidTransaction(format!(
            "{}",
            invalid_transaction
        )))
    }
}
