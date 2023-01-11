use std::{array::TryFromSliceError, error::Error as StdError};

use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;

use casper_types::{TimeDiff, U512};

/// A representation of the way in which a deploy failed validation checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub enum DeployConfigurationFailure {
    /// Invalid chain name.
    #[error("invalid chain name: expected {expected}, got {got}")]
    InvalidChainName {
        /// The expected chain name.
        expected: String,
        /// The received chain name.
        got: String,
    },

    /// Too many dependencies.
    #[error("{got} dependencies exceeds limit of {max_dependencies}")]
    ExcessiveDependencies {
        /// The dependencies limit.
        max_dependencies: u8,
        /// The actual number of dependencies provided.
        got: usize,
    },

    /// Deploy is too large.
    #[error("deploy size too large: {0}")]
    ExcessiveSize(#[from] ExcessiveSizeError),

    /// Excessive time-to-live.
    #[error("time-to-live of {got} exceeds limit of {max_ttl}")]
    ExcessiveTimeToLive {
        /// The time-to-live limit.
        max_ttl: TimeDiff,
        /// The received time-to-live.
        got: TimeDiff,
    },

    /// The provided body hash does not match the actual hash of the body.
    #[error("the provided body hash does not match the actual hash of the body")]
    InvalidBodyHash,

    /// The provided deploy hash does not match the actual hash of the deploy.
    #[error("the provided hash does not match the actual hash of the deploy")]
    InvalidDeployHash,

    /// The deploy has no approvals.
    #[error("the deploy has no approvals")]
    EmptyApprovals,

    /// Invalid approval.
    #[error("the approval at index {index} is invalid: {error_msg}")]
    InvalidApproval {
        /// The index of the approval at fault.
        index: usize,
        /// The approval validation error.
        error_msg: String,
    },

    /// Excessive length of deploy's session args.
    #[error("serialized session code runtime args of {got} exceeds limit of {max_length}")]
    ExcessiveSessionArgsLength {
        /// The byte size limit of session arguments.
        max_length: usize,
        /// The received length of session arguments.
        got: usize,
    },

    /// Excessive length of deploy's payment args.
    #[error("serialized payment code runtime args of {got} exceeds limit of {max_length}")]
    ExcessivePaymentArgsLength {
        /// The byte size limit of payment arguments.
        max_length: usize,
        /// The received length of payment arguments.
        got: usize,
    },

    /// Missing payment "amount" runtime argument.
    #[error("missing payment 'amount' runtime argument ")]
    MissingPaymentAmount,

    /// Failed to parse payment "amount" runtime argument.
    #[error("failed to parse payment 'amount' as U512")]
    FailedToParsePaymentAmount,

    /// The payment amount associated with the deploy exceeds the block gas limit.
    #[error("payment amount of {got} exceeds the block gas limit of {block_gas_limit}")]
    ExceededBlockGasLimit {
        /// Configured block gas limit.
        block_gas_limit: u64,
        /// The payment amount received.
        got: U512,
    },

    /// Missing payment "amount" runtime argument
    #[error("missing transfer 'amount' runtime argument")]
    MissingTransferAmount,

    /// Failed to parse transfer "amount" runtime argument.
    #[error("failed to parse transfer 'amount' as U512")]
    FailedToParseTransferAmount,

    /// Insufficient transfer amount.
    #[error("insufficient transfer amount; minimum: {minimum} attempted: {attempted}")]
    InsufficientTransferAmount {
        /// The minimum transfer amount.
        minimum: U512,
        /// The attempted transfer amount.
        attempted: U512,
    },

    /// The amount of approvals on the deploy exceeds the max_associated_keys limit.
    #[error("number of associated keys {got} exceeds the maximum {max_associated_keys}")]
    ExcessiveApprovals {
        /// Number of approvals on the deploy.
        got: u32,
        /// The chainspec limit for max_associated_keys.
        max_associated_keys: u32,
    },
}

/// Error returned when a Deploy is too large.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
#[error("deploy size of {actual_deploy_size} bytes exceeds limit of {max_deploy_size}")]
pub struct ExcessiveSizeError {
    /// The maximum permitted serialized deploy size, in bytes.
    pub max_deploy_size: u32,
    /// The serialized size of the deploy provided, in bytes.
    pub actual_deploy_size: usize,
}

/// Errors other than validation failures relating to `Deploy`s.
#[derive(Debug, Error)]
pub enum Error {
    /// Error while encoding to JSON.
    #[error("encoding to JSON: {0}")]
    EncodeToJson(#[from] serde_json::Error),

    /// Error while decoding from JSON.
    #[error("decoding from JSON: {0}")]
    DecodeFromJson(Box<dyn StdError>),

    /// Failed to get "amount" from `payment()`'s runtime args.
    #[error("invalid payment: missing \"amount\" arg")]
    InvalidPayment,
}

impl From<base16::DecodeError> for Error {
    fn from(error: base16::DecodeError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}

impl From<TryFromSliceError> for Error {
    fn from(error: TryFromSliceError) -> Self {
        Error::DecodeFromJson(Box::new(error))
    }
}
