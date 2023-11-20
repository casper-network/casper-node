use crate::node_client::Error as NodeClientError;
use casper_json_rpc::Error as RpcError;
use casper_types::{BlockHash, Digest, TransactionHash};

use super::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request for {0} has failed: {1}")]
    NodeRequest(&'static str, NodeClientError),
    #[error("no block for hash {0}")]
    NoBlockWithHash(BlockHash),
    #[error("no block at height {0}")]
    NoBlockAtHeight(u64),
    #[error("no highest block found")]
    NoHighestBlock,
    #[error("no block body for hash {0}")]
    NoBlockBodyWithHash(Digest),
    #[error("could not verify block with hash {0}")]
    CouldNotVerifyBlock(BlockHash),
    #[error("no transaction for hash {0}")]
    NoTransactionWithHash(TransactionHash),
    #[error("transaction {0} and its approval versions are inconsistent")]
    InconsistentTransactionVersions(TransactionHash),
    #[error("found a transaction when searching for a deploy")]
    FoundTransactionInsteadOfDeploy,
}

impl Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::NodeRequest(_, _) => ErrorCode::NodeRequestFailed,
            Error::NoBlockWithHash(_)
            | Error::NoBlockAtHeight(_)
            | Error::NoHighestBlock
            | Error::NoBlockBodyWithHash(_) => ErrorCode::NoSuchBlock,
            Error::CouldNotVerifyBlock(_) => ErrorCode::InvalidBlock,
            Error::NoTransactionWithHash(_) => ErrorCode::NoSuchTransaction,
            Error::InconsistentTransactionVersions(_) | Error::FoundTransactionInsteadOfDeploy => {
                ErrorCode::VariantMismatch
            }
        }
    }
}

impl From<Error> for RpcError {
    fn from(value: Error) -> Self {
        RpcError::new(value.code(), value.to_string())
    }
}
