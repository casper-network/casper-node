use crate::node_client::Error as NodeClientError;
use casper_json_rpc::Error as RpcError;
use casper_types::{
    AvailableBlockRange, BlockHash, DeployHash, Digest, KeyFromStrError, KeyTag, TransactionHash,
    URefFromStrError,
};

use super::{ErrorCode, ErrorData};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request for {0} has failed: {1}")]
    NodeRequest(&'static str, NodeClientError),
    #[error("no block for hash {0}")]
    NoBlockWithHash(BlockHash, AvailableBlockRange),
    #[error("no block body for hash {0}")]
    NoBlockBodyWithHash(Digest, AvailableBlockRange),
    #[error("no block at height {0}")]
    NoBlockAtHeight(u64, AvailableBlockRange),
    #[error("no highest block found")]
    NoHighestBlock(AvailableBlockRange),
    #[error("could not verify block with hash {0}")]
    CouldNotVerifyBlock(BlockHash),
    #[error("no transaction for hash {0}")]
    NoTransactionWithHash(TransactionHash),
    #[error("no deploy for hash {0}")]
    NoDeployWithHash(DeployHash),
    #[error("transaction {0} and its approval versions are inconsistent")]
    InconsistentTransactionVersions(TransactionHash),
    #[error("found a transaction when searching for a deploy")]
    FoundTransactionInsteadOfDeploy,
    #[error("value was not found in the global state")]
    GlobalStateEntryNotFound,
    #[error("global state root hash not found")]
    GlobalStateRootHashNotFound,
    #[error("global state query has failed: {0}")]
    GlobalStateQueryFailed(String),
    #[error("the requested purse URef was invalid: {0}")]
    InvalidPurseURef(URefFromStrError),
    #[error("the requested purse balance could not be parsed")]
    InvalidPurseBalance,
    #[error("the requested main purse was invalid")]
    InvalidMainPurse,
    #[error("the requested account info could not be parsed")]
    InvalidAccountInfo,
    #[error("the provided dictionary key was invalid: {0}")]
    InvalidDictionaryKey(KeyFromStrError),
    #[error("the provided dictionary key points at an unexpected type: {0}")]
    InvalidTypeUnderDictionaryKey(String),
    #[error("the provided dictionary key doesn't exist")]
    DictionaryKeyNotFound,
    #[error("the provided dictionary name doesn't exist")]
    DictionaryNameNotFound,
    #[error("the provided dictionary value is {0} instead of a URef")]
    DictionaryValueIsNotAUref(KeyTag),
    #[error("the provided dictionary key could not be parsed: {0}")]
    DictionaryKeyCouldNotBeParsed(String),
    #[error("the transaction was invalid: {0}")]
    InvalidTransaction(String),
    #[error("the deploy was invalid: {0}")]
    InvalidDeploy(String),
    #[error("the peers response was invalid: {0}")]
    InvalidPeersResponse(String),
    #[error("the auction bids were invalid")]
    InvalidAuctionBids,
    #[error("the auction contract was invalid")]
    InvalidAuctionContract,
    #[error("the auction validators were invalid")]
    InvalidAuctionValidators,
    #[error("speculative execution returned nothing")]
    SpecExecReturnedNothing,
}

impl Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::NodeRequest(_, _) | Error::InvalidPeersResponse(_) => {
                ErrorCode::NodeRequestFailed
            }
            Error::NoBlockWithHash(_, _)
            | Error::NoBlockAtHeight(_, _)
            | Error::NoHighestBlock(_)
            | Error::NoBlockBodyWithHash(_, _) => ErrorCode::NoSuchBlock,
            Error::CouldNotVerifyBlock(_) => ErrorCode::InvalidBlock,
            Error::NoTransactionWithHash(_) => ErrorCode::NoSuchTransaction,
            Error::NoDeployWithHash(_) => ErrorCode::NoSuchDeploy,
            Error::InconsistentTransactionVersions(_) | Error::FoundTransactionInsteadOfDeploy => {
                ErrorCode::VariantMismatch
            }
            Error::GlobalStateRootHashNotFound => ErrorCode::NoSuchStateRoot,
            Error::GlobalStateQueryFailed(_) | Error::GlobalStateEntryNotFound => {
                ErrorCode::QueryFailed
            }
            Error::InvalidPurseURef(_) => ErrorCode::FailedToParseGetBalanceURef,
            Error::InvalidPurseBalance => ErrorCode::FailedToGetBalance,
            Error::InvalidAccountInfo => ErrorCode::NoSuchAccount,
            Error::InvalidDictionaryKey(_) => ErrorCode::FailedToParseQueryKey,
            Error::InvalidMainPurse => ErrorCode::NoSuchMainPurse,
            Error::InvalidTypeUnderDictionaryKey(_)
            | Error::DictionaryKeyNotFound
            | Error::DictionaryNameNotFound
            | Error::DictionaryValueIsNotAUref(_)
            | Error::DictionaryKeyCouldNotBeParsed(_) => ErrorCode::FailedToGetDictionaryURef,
            Error::InvalidTransaction(_) => ErrorCode::InvalidTransaction,
            Error::InvalidDeploy(_) | Error::SpecExecReturnedNothing => ErrorCode::InvalidDeploy,
            Error::InvalidAuctionBids
            | Error::InvalidAuctionContract
            | Error::InvalidAuctionValidators => ErrorCode::InvalidAuctionState,
        }
    }
}

impl From<Error> for RpcError {
    fn from(value: Error) -> Self {
        match value {
            Error::NoBlockWithHash(_, available_block_range)
            | Error::NoBlockBodyWithHash(_, available_block_range)
            | Error::NoBlockAtHeight(_, available_block_range)
            | Error::NoHighestBlock(available_block_range) => RpcError::new(
                value.code(),
                ErrorData::MissingBlockOrStateRoot {
                    message: value.to_string(),
                    available_block_range,
                },
            ),
            _ => RpcError::new(value.code(), value.to_string()),
        }
    }
}
