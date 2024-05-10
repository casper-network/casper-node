use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;

use casper_binary_port::ErrorCode as BinaryPortErrorCode;
use casper_types::{
    AddressableEntityHash, BlockHash, BlockHeader, Digest, EntityVersion, InitiatorAddr,
    InvalidTransaction, PackageHash, Timestamp,
};

// `allow` can be removed once https://github.com/casper-network/casper-node/issues/3063 is fixed.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Error, Serialize)]
pub(crate) enum Error {
    /// The block chain has no blocks.
    #[error("block chain has no blocks")]
    EmptyBlockchain,

    /// The deploy has an invalid transaction.
    #[error("invalid transaction: {0}")]
    InvalidTransaction(#[from] InvalidTransaction),

    /// The transaction is invalid due to missing or otherwise invalid parameters.
    #[error(
        "{failure} at state root hash {:?} of block {:?} at height {block_height}",
        state_root_hash,
        block_hash.inner(),
    )]
    Parameters {
        state_root_hash: Digest,
        block_hash: BlockHash,
        block_height: u64,
        failure: ParameterFailure,
    },

    /// The transaction received by the node from the client has expired.
    #[error(
        "transaction received by the node expired at {expiry_timestamp} with node's time at \
        {current_node_timestamp}"
    )]
    Expired {
        /// The timestamp when the transaction expired.
        expiry_timestamp: Timestamp,
        /// The timestamp when the node validated the expiry timestamp.
        current_node_timestamp: Timestamp,
    },

    /// Component state error: expected a deploy.
    #[error("internal error: expected a deploy")]
    ExpectedDeploy,

    /// Component state error: expected a version 1 transaction.
    #[error("internal error: expected a transaction")]
    ExpectedTransactionV1,
}

impl Error {
    pub(super) fn parameter_failure(block_header: &BlockHeader, failure: ParameterFailure) -> Self {
        Error::Parameters {
            state_root_hash: *block_header.state_root_hash(),
            block_hash: block_header.block_hash(),
            block_height: block_header.height(),
            failure,
        }
    }
}

impl From<Error> for BinaryPortErrorCode {
    fn from(err: Error) -> Self {
        match err {
            Error::EmptyBlockchain
            | Error::Parameters { .. }
            | Error::Expired { .. }
            | Error::ExpectedDeploy
            | Error::ExpectedTransactionV1 => {
                BinaryPortErrorCode::InvalidTransactionOrDeployUnspecified
            }
            Error::InvalidTransaction(invalid_transaction) => {
                BinaryPortErrorCode::from(invalid_transaction)
            }
        }
    }
}

/// A representation of the way in which a transaction failed parameter checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub(crate) enum ParameterFailure {
    /// No such addressable entity.
    #[error("addressable entity under {initiator_addr} does not exist")]
    NoSuchAddressableEntity { initiator_addr: InitiatorAddr },
    /// No such contract at given hash.
    #[error("contract at {contract_hash} does not exist")]
    NoSuchContractAtHash {
        contract_hash: AddressableEntityHash,
    },
    /// No such contract entrypoint.
    #[error("contract does not have entry point '{entry_point_name}'")]
    NoSuchEntryPoint { entry_point_name: String },
    /// No such package.
    #[error("package at {package_hash} does not exist")]
    NoSuchPackageAtHash { package_hash: PackageHash },
    /// Invalid contract at given version.
    #[error("invalid entity at version: {entity_version}")]
    InvalidEntityAtVersion { entity_version: EntityVersion },
    /// Invalid contract at given version.
    #[error("disabled entity at version: {entity_version}")]
    DisabledEntityAtVersion { entity_version: EntityVersion },
    /// Invalid contract at given version.
    #[error("missing entity at version: {entity_version}")]
    MissingEntityAtVersion { entity_version: EntityVersion },
    /// Invalid associated keys.
    #[error("account authorization invalid")]
    InvalidAssociatedKeys,
    /// Insufficient transaction signature weight.
    #[error("insufficient transaction signature weight")]
    InsufficientSignatureWeight,
    /// The transaction's addressable entity has insufficient balance.
    #[error("insufficient balance in {initiator_addr}")]
    InsufficientBalance { initiator_addr: InitiatorAddr },
    /// The balance of the transaction's addressable entity cannot be read.
    #[error("unable to determine balance for {initiator_addr}")]
    UnknownBalance { initiator_addr: InitiatorAddr },
    /// Error specific to `Deploy` parameters.
    #[error(transparent)]
    Deploy(#[from] DeployParameterFailure),
}

/// A representation of the way in which a deploy failed validation checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub(crate) enum DeployParameterFailure {
    /// Transfer is not valid for payment code.
    #[error("transfer is not valid for payment code")]
    InvalidPaymentVariant,
    /// Missing payment "amount" runtime argument.
    #[error("missing payment 'amount' runtime argument")]
    MissingPaymentAmount,
    /// Failed to parse payment "amount" runtime argument.
    #[error("failed to parse payment 'amount' runtime argument as U512")]
    FailedToParsePaymentAmount,
    /// Missing transfer "target" runtime argument.
    #[error("missing transfer 'target' runtime argument")]
    MissingTransferTarget,
    /// Module bytes for session code cannot be empty.
    #[error("module bytes for session code cannot be empty")]
    MissingModuleBytes,
}
