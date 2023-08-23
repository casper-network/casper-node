use alloc::string::String;
use core::{
    array::TryFromSliceError,
    fmt::{self, Display, Formatter},
};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::Serialize;

use crate::{crypto, TimeDiff, Timestamp};

/// Returned when a `Transaction` fails validation.
#[derive(Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "std", derive(Serialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum TransactionConfigFailure {
    /// Invalid chain name.
    InvalidChainName {
        /// The expected chain name.
        expected: String,
        /// The `Transaction`'s chain name.
        got: String,
    },

    /// `Transaction` is too large.
    ExcessiveSize(ExcessiveSizeError),

    /// Excessive time-to-live.
    ExcessiveTimeToLive {
        /// The time-to-live limit.
        max_ttl: TimeDiff,
        /// The `Transaction`'s time-to-live.
        got: TimeDiff,
    },

    /// `Transaction`'s timestamp is in the future.
    TimestampInFuture {
        /// The node's timestamp when validating the `Transaction`.
        validation_timestamp: Timestamp,
        /// The `Transaction`'s timestamp.
        got: Timestamp,
    },

    /// The provided body hash does not match the actual hash of the body.
    InvalidBodyHash,

    /// The provided `Transaction` hash does not match the actual hash of the `Transaction`.
    InvalidTransactionHash,

    /// The `Transaction` has no approvals.
    EmptyApprovals,

    /// Invalid approval.
    InvalidApproval {
        /// The index of the approval at fault.
        index: usize,
        /// The approval verification error.
        error: crypto::Error,
    },

    /// Excessive length of `Transaction`'s runtime args.
    ExcessiveArgsLength {
        /// The byte size limit of runtime arguments.
        max_length: usize,
        /// The length of the `Transaction`'s runtime arguments.
        got: usize,
    },

    /// The amount of approvals on the `Transaction` exceeds the configured limit.
    ExcessiveApprovals {
        /// The chainspec limit for max_associated_keys.
        max_associated_keys: u32,
        /// Number of approvals on the `Transaction`.
        got: u32,
    },
}

impl Display for TransactionConfigFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionConfigFailure::InvalidChainName { expected, got } => {
                write!(
                    formatter,
                    "invalid chain name: expected {}, got {}",
                    expected, got
                )
            }
            TransactionConfigFailure::ExcessiveSize(error) => {
                write!(formatter, "transaction size too large: {}", error)
            }
            TransactionConfigFailure::ExcessiveTimeToLive { max_ttl, got } => {
                write!(
                    formatter,
                    "time-to-live of {} exceeds limit of {}",
                    got, max_ttl
                )
            }
            TransactionConfigFailure::TimestampInFuture {
                validation_timestamp,
                got,
            } => {
                write!(
                    formatter,
                    "timestamp of {} is later than node's validation timestamp of {}",
                    got, validation_timestamp
                )
            }
            TransactionConfigFailure::InvalidBodyHash => {
                write!(
                    formatter,
                    "the provided hash does not match the actual hash of the transaction body"
                )
            }
            TransactionConfigFailure::InvalidTransactionHash => {
                write!(
                    formatter,
                    "the provided hash does not match the actual hash of the transaction"
                )
            }
            TransactionConfigFailure::EmptyApprovals => {
                write!(formatter, "the transaction has no approvals")
            }
            TransactionConfigFailure::InvalidApproval { index, error } => {
                write!(
                    formatter,
                    "the transaction approval at index {} is invalid: {}",
                    index, error
                )
            }
            TransactionConfigFailure::ExcessiveArgsLength { max_length, got } => {
                write!(
                    formatter,
                    "serialized transaction runtime args of {} bytes exceeds limit of {} bytes",
                    got, max_length
                )
            }
            TransactionConfigFailure::ExcessiveApprovals {
                max_associated_keys,
                got,
            } => {
                write!(
                    formatter,
                    "number of transaction approvals {} exceeds the maximum number of associated \
                    keys {}",
                    got, max_associated_keys
                )
            }
        }
    }
}

impl From<ExcessiveSizeError> for TransactionConfigFailure {
    fn from(error: ExcessiveSizeError) -> Self {
        TransactionConfigFailure::ExcessiveSize(error)
    }
}

#[cfg(feature = "std")]
impl StdError for TransactionConfigFailure {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            TransactionConfigFailure::InvalidApproval { error, .. } => Some(error),
            TransactionConfigFailure::InvalidChainName { .. }
            | TransactionConfigFailure::ExcessiveSize(_)
            | TransactionConfigFailure::ExcessiveTimeToLive { .. }
            | TransactionConfigFailure::TimestampInFuture { .. }
            | TransactionConfigFailure::InvalidBodyHash
            | TransactionConfigFailure::InvalidTransactionHash
            | TransactionConfigFailure::EmptyApprovals
            | TransactionConfigFailure::ExcessiveArgsLength { .. }
            | TransactionConfigFailure::ExcessiveApprovals { .. } => None,
        }
    }
}

/// Error returned when a `Transaction` is too large.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ExcessiveSizeError {
    /// The maximum permitted serialized `Transaction` size, in bytes.
    pub max_transaction_size: u32,
    /// The serialized size of the `Transaction` provided, in bytes.
    pub actual_transaction_size: usize,
}

impl Display for ExcessiveSizeError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction size of {} bytes exceeds limit of {}",
            self.actual_transaction_size, self.max_transaction_size
        )
    }
}

#[cfg(feature = "std")]
impl StdError for ExcessiveSizeError {}

/// Errors other than validation failures relating to Transactions.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Error while encoding to JSON.
    EncodeToJson(serde_json::Error),

    /// Error while decoding from JSON.
    DecodeFromJson(DecodeFromJsonError),
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
        }
    }
}

#[cfg(feature = "std")]
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::EncodeToJson(error) => Some(error),
            Error::DecodeFromJson(error) => Some(error),
        }
    }
}

/// Error while decoding a `Transaction` from JSON.
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
