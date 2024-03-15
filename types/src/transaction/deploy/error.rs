use alloc::{boxed::Box, string::String};
use core::{
    array::TryFromSliceError,
    fmt::{self, Display, Formatter},
};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::Serialize;

use crate::{crypto, TimeDiff, Timestamp, U512};

/// A representation of the way in which a deploy failed validation checks.
#[derive(Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "std", derive(Serialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum InvalidDeploy {
    /// Invalid chain name.
    InvalidChainName {
        /// The expected chain name.
        expected: String,
        /// The received chain name.
        got: String,
    },

    /// Deploy dependencies are no longer supported.
    DependenciesNoLongerSupported,

    /// Deploy is too large.
    ExcessiveSize(ExcessiveSizeError),

    /// Excessive time-to-live.
    ExcessiveTimeToLive {
        /// The time-to-live limit.
        max_ttl: TimeDiff,
        /// The received time-to-live.
        got: TimeDiff,
    },

    /// Deploy's timestamp is in the future.
    TimestampInFuture {
        /// The node's timestamp when validating the deploy.
        validation_timestamp: Timestamp,
        /// Any configured leeway added to `validation_timestamp`.
        timestamp_leeway: TimeDiff,
        /// The deploy's timestamp.
        got: Timestamp,
    },

    /// The provided body hash does not match the actual hash of the body.
    InvalidBodyHash,

    /// The provided deploy hash does not match the actual hash of the deploy.
    InvalidDeployHash,

    /// The deploy has no approvals.
    EmptyApprovals,

    /// Invalid approval.
    InvalidApproval {
        /// The index of the approval at fault.
        index: usize,
        /// The approval verification error.
        error: crypto::Error,
    },

    /// Excessive length of deploy's session args.
    ExcessiveSessionArgsLength {
        /// The byte size limit of session arguments.
        max_length: usize,
        /// The received length of session arguments.
        got: usize,
    },

    /// Excessive length of deploy's payment args.
    ExcessivePaymentArgsLength {
        /// The byte size limit of payment arguments.
        max_length: usize,
        /// The received length of payment arguments.
        got: usize,
    },

    /// Missing payment "amount" runtime argument.
    MissingPaymentAmount,

    /// Failed to parse payment "amount" runtime argument.
    FailedToParsePaymentAmount,

    /// The payment amount associated with the deploy exceeds the block gas limit.
    ExceededBlockGasLimit {
        /// Configured block gas limit.
        block_gas_limit: u64,
        /// The payment amount received.
        got: Box<U512>,
    },

    /// Missing payment "amount" runtime argument
    MissingTransferAmount,

    /// Failed to parse transfer "amount" runtime argument.
    FailedToParseTransferAmount,

    /// Insufficient transfer amount.
    InsufficientTransferAmount {
        /// The minimum transfer amount.
        minimum: Box<U512>,
        /// The attempted transfer amount.
        attempted: Box<U512>,
    },

    /// The amount of approvals on the deploy exceeds the max_associated_keys limit.
    ExcessiveApprovals {
        /// Number of approvals on the deploy.
        got: u32,
        /// The chainspec limit for max_associated_keys.
        max_associated_keys: u32,
    },
}

impl Display for InvalidDeploy {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            InvalidDeploy::InvalidChainName { expected, got } => {
                write!(
                    formatter,
                    "invalid chain name: expected {}, got {}",
                    expected, got
                )
            }
            InvalidDeploy::DependenciesNoLongerSupported => {
                write!(formatter, "dependencies no longer supported",)
            }
            InvalidDeploy::ExcessiveSize(error) => {
                write!(formatter, "deploy size too large: {}", error)
            }
            InvalidDeploy::ExcessiveTimeToLive { max_ttl, got } => {
                write!(
                    formatter,
                    "time-to-live of {} exceeds limit of {}",
                    got, max_ttl
                )
            }
            InvalidDeploy::TimestampInFuture {
                validation_timestamp,
                timestamp_leeway,
                got,
            } => {
                write!(
                    formatter,
                    "timestamp of {} is later than node's timestamp of {} plus leeway of {}",
                    got, validation_timestamp, timestamp_leeway
                )
            }
            InvalidDeploy::InvalidBodyHash => {
                write!(
                    formatter,
                    "the provided body hash does not match the actual hash of the body"
                )
            }
            InvalidDeploy::InvalidDeployHash => {
                write!(
                    formatter,
                    "the provided hash does not match the actual hash of the deploy"
                )
            }
            InvalidDeploy::EmptyApprovals => {
                write!(formatter, "the deploy has no approvals")
            }
            InvalidDeploy::InvalidApproval { index, error } => {
                write!(
                    formatter,
                    "the approval at index {} is invalid: {}",
                    index, error
                )
            }
            InvalidDeploy::ExcessiveSessionArgsLength { max_length, got } => {
                write!(
                    formatter,
                    "serialized session code runtime args of {} exceeds limit of {}",
                    got, max_length
                )
            }
            InvalidDeploy::ExcessivePaymentArgsLength { max_length, got } => {
                write!(
                    formatter,
                    "serialized payment code runtime args of {} exceeds limit of {}",
                    got, max_length
                )
            }
            InvalidDeploy::MissingPaymentAmount => {
                write!(formatter, "missing payment 'amount' runtime argument")
            }
            InvalidDeploy::FailedToParsePaymentAmount => {
                write!(formatter, "failed to parse payment 'amount' as U512")
            }
            InvalidDeploy::ExceededBlockGasLimit {
                block_gas_limit,
                got,
            } => {
                write!(
                    formatter,
                    "payment amount of {} exceeds the block gas limit of {}",
                    got, block_gas_limit
                )
            }
            InvalidDeploy::MissingTransferAmount => {
                write!(formatter, "missing transfer 'amount' runtime argument")
            }
            InvalidDeploy::FailedToParseTransferAmount => {
                write!(formatter, "failed to parse transfer 'amount' as U512")
            }
            InvalidDeploy::InsufficientTransferAmount { minimum, attempted } => {
                write!(
                    formatter,
                    "insufficient transfer amount; minimum: {} attempted: {}",
                    minimum, attempted
                )
            }
            InvalidDeploy::ExcessiveApprovals {
                got,
                max_associated_keys,
            } => {
                write!(
                    formatter,
                    "number of approvals {} exceeds the maximum number of associated keys {}",
                    got, max_associated_keys
                )
            }
        }
    }
}

impl From<ExcessiveSizeError> for InvalidDeploy {
    fn from(error: ExcessiveSizeError) -> Self {
        InvalidDeploy::ExcessiveSize(error)
    }
}

#[cfg(feature = "std")]
impl StdError for InvalidDeploy {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            InvalidDeploy::InvalidApproval { error, .. } => Some(error),
            InvalidDeploy::InvalidChainName { .. }
            | InvalidDeploy::DependenciesNoLongerSupported { .. }
            | InvalidDeploy::ExcessiveSize(_)
            | InvalidDeploy::ExcessiveTimeToLive { .. }
            | InvalidDeploy::TimestampInFuture { .. }
            | InvalidDeploy::InvalidBodyHash
            | InvalidDeploy::InvalidDeployHash
            | InvalidDeploy::EmptyApprovals
            | InvalidDeploy::ExcessiveSessionArgsLength { .. }
            | InvalidDeploy::ExcessivePaymentArgsLength { .. }
            | InvalidDeploy::MissingPaymentAmount
            | InvalidDeploy::FailedToParsePaymentAmount
            | InvalidDeploy::ExceededBlockGasLimit { .. }
            | InvalidDeploy::MissingTransferAmount
            | InvalidDeploy::FailedToParseTransferAmount
            | InvalidDeploy::InsufficientTransferAmount { .. }
            | InvalidDeploy::ExcessiveApprovals { .. } => None,
        }
    }
}

/// Error returned when a Deploy is too large.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ExcessiveSizeError {
    /// The maximum permitted serialized deploy size, in bytes.
    pub max_transaction_size: u32,
    /// The serialized size of the deploy provided, in bytes.
    pub actual_deploy_size: usize,
}

impl Display for ExcessiveSizeError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "deploy size of {} bytes exceeds limit of {}",
            self.actual_deploy_size, self.max_transaction_size
        )
    }
}

#[cfg(feature = "std")]
impl StdError for ExcessiveSizeError {}
/// Errors other than validation failures relating to `Deploy`s.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Error while encoding to JSON.
    EncodeToJson(serde_json::Error),

    /// Error while decoding from JSON.
    DecodeFromJson(DecodeFromJsonError),

    /// Failed to get "amount" from `payment()`'s runtime args.
    InvalidPayment,
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::EncodeToJson(error)
    }
}

impl From<DecodeFromJsonError> for Error {
    fn from(error: DecodeFromJsonError) -> Self {
        Error::DecodeFromJson(error)
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Error::EncodeToJson(error) => {
                write!(formatter, "encoding to json: {}", error)
            }
            Error::DecodeFromJson(error) => {
                write!(formatter, "decoding from json: {}", error)
            }
            Error::InvalidPayment => {
                write!(formatter, "invalid payment: missing 'amount' arg")
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::EncodeToJson(error) => Some(error),
            Error::DecodeFromJson(error) => Some(error),
            Error::InvalidPayment => None,
        }
    }
}

/// Error while decoding a `Deploy` from JSON.
#[derive(Debug)]
#[non_exhaustive]
pub enum DecodeFromJsonError {
    /// Failed to decode from base 16.
    FromHex(base16::DecodeError),

    /// Failed to convert slice to array.
    TryFromSlice(TryFromSliceError),
}

impl From<base16::DecodeError> for DecodeFromJsonError {
    fn from(error: base16::DecodeError) -> Self {
        DecodeFromJsonError::FromHex(error)
    }
}

impl From<TryFromSliceError> for DecodeFromJsonError {
    fn from(error: TryFromSliceError) -> Self {
        DecodeFromJsonError::TryFromSlice(error)
    }
}

impl Display for DecodeFromJsonError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            DecodeFromJsonError::FromHex(error) => {
                write!(formatter, "{}", error)
            }
            DecodeFromJsonError::TryFromSlice(error) => {
                write!(formatter, "{}", error)
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for DecodeFromJsonError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            DecodeFromJsonError::FromHex(error) => Some(error),
            DecodeFromJsonError::TryFromSlice(error) => Some(error),
        }
    }
}
