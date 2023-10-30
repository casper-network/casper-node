use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;

use casper_types::{
    AddressableEntityHash, BlockHash, BlockHeader, DeployConfigFailure, Digest, EntityVersion,
    PackageHash, PublicKey, Timestamp, TransactionV1ConfigFailure,
};

// `allow` can be removed once https://github.com/casper-network/casper-node/issues/3063 is fixed.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Error, Serialize)]
pub(crate) enum Error {
    /// The block chain has no blocks.
    #[error("block chain has no blocks")]
    EmptyBlockchain,

    /// The deploy has an invalid configuration.
    #[error("invalid deploy: {0}")]
    InvalidDeployConfiguration(#[from] DeployConfigFailure),

    /// The v1 transaction has an invalid configuration.
    #[error("invalid v1 transaction: {0}")]
    InvalidV1Configuration(#[from] TransactionV1ConfigFailure),

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
    #[error("internal error: expected a deploy")]
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

/// A representation of the way in which a transaction failed parameter checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub(crate) enum ParameterFailure {
    /// No such addressable entity.
    #[error("addressable entity under {public_key} does not exist")]
    NoSuchAddressableEntity { public_key: PublicKey },
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
    #[error("invalid contract at version: {contract_version}")]
    InvalidContractAtVersion { contract_version: EntityVersion },
    /// Invalid associated keys.
    #[error("account authorization invalid")]
    InvalidAssociatedKeys,
    /// Insufficient transaction signature weight.
    #[error("insufficient transaction signature weight")]
    InsufficientSignatureWeight,
    /// The transaction's addressable entity has insufficient balance.
    #[error("insufficient balance in {public_key}")]
    InsufficientBalance { public_key: PublicKey },
    /// The balance of the transaction's addressable entity cannot be read.
    #[error("unable to determine balance for {public_key}")]
    UnknownBalance { public_key: PublicKey },
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
