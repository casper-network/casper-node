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

#[cfg(doc)]
use super::TransactionV1;
use crate::{crypto, TimeDiff, Timestamp};

/// Returned when a [`TransactionV1`] fails validation.
#[derive(Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "std", derive(Serialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum TransactionV1ConfigFailure {
    /// Invalid chain name.
    InvalidChainName {
        /// The expected chain name.
        expected: String,
        /// The transaction's chain name.
        got: String,
    },

    /// Transaction is too large.
    ExcessiveSize(ExcessiveSizeErrorV1),

    /// Excessive time-to-live.
    ExcessiveTimeToLive {
        /// The time-to-live limit.
        max_ttl: TimeDiff,
        /// The transaction's time-to-live.
        got: TimeDiff,
    },

    /// Transaction's timestamp is in the future.
    TimestampInFuture {
        /// The node's timestamp when validating the transaction.
        validation_timestamp: Timestamp,
        /// The transaction's timestamp.
        got: Timestamp,
    },

    /// The provided body hash does not match the actual hash of the body.
    InvalidBodyHash,

    /// The provided transaction hash does not match the actual hash of the transaction.
    InvalidTransactionHash,

    /// The transaction has no approvals.
    EmptyApprovals,

    /// Invalid approval.
    InvalidApproval {
        /// The index of the approval at fault.
        index: usize,
        /// The approval verification error.
        error: crypto::Error,
    },

    /// Excessive length of transaction's runtime args.
    ExcessiveArgsLength {
        /// The byte size limit of runtime arguments.
        max_length: usize,
        /// The length of the transaction's runtime arguments.
        got: usize,
    },

    /// The amount of approvals on the transaction exceeds the configured limit.
    ExcessiveApprovals {
        /// The chainspec limit for max_associated_keys.
        max_associated_keys: u32,
        /// Number of approvals on the transaction.
        got: u32,
    },
}

impl Display for TransactionV1ConfigFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionV1ConfigFailure::InvalidChainName { expected, got } => {
                write!(
                    formatter,
                    "invalid chain name: expected {}, got {}",
                    expected, got
                )
            }
            TransactionV1ConfigFailure::ExcessiveSize(error) => {
                write!(formatter, "transaction size too large: {}", error)
            }
            TransactionV1ConfigFailure::ExcessiveTimeToLive { max_ttl, got } => {
                write!(
                    formatter,
                    "time-to-live of {} exceeds limit of {}",
                    got, max_ttl
                )
            }
            TransactionV1ConfigFailure::TimestampInFuture {
                validation_timestamp,
                got,
            } => {
                write!(
                    formatter,
                    "timestamp of {} is later than node's validation timestamp of {}",
                    got, validation_timestamp
                )
            }
            TransactionV1ConfigFailure::InvalidBodyHash => {
                write!(
                    formatter,
                    "the provided hash does not match the actual hash of the transaction body"
                )
            }
            TransactionV1ConfigFailure::InvalidTransactionHash => {
                write!(
                    formatter,
                    "the provided hash does not match the actual hash of the transaction"
                )
            }
            TransactionV1ConfigFailure::EmptyApprovals => {
                write!(formatter, "the transaction has no approvals")
            }
            TransactionV1ConfigFailure::InvalidApproval { index, error } => {
                write!(
                    formatter,
                    "the transaction approval at index {} is invalid: {}",
                    index, error
                )
            }
            TransactionV1ConfigFailure::ExcessiveArgsLength { max_length, got } => {
                write!(
                    formatter,
                    "serialized transaction runtime args of {} bytes exceeds limit of {} bytes",
                    got, max_length
                )
            }
            TransactionV1ConfigFailure::ExcessiveApprovals {
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

impl From<ExcessiveSizeErrorV1> for TransactionV1ConfigFailure {
    fn from(error: ExcessiveSizeErrorV1) -> Self {
        TransactionV1ConfigFailure::ExcessiveSize(error)
    }
}

#[cfg(feature = "std")]
impl StdError for TransactionV1ConfigFailure {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            TransactionV1ConfigFailure::InvalidApproval { error, .. } => Some(error),
            TransactionV1ConfigFailure::InvalidChainName { .. }
            | TransactionV1ConfigFailure::ExcessiveSize(_)
            | TransactionV1ConfigFailure::ExcessiveTimeToLive { .. }
            | TransactionV1ConfigFailure::TimestampInFuture { .. }
            | TransactionV1ConfigFailure::InvalidBodyHash
            | TransactionV1ConfigFailure::InvalidTransactionHash
            | TransactionV1ConfigFailure::EmptyApprovals
            | TransactionV1ConfigFailure::ExcessiveArgsLength { .. }
            | TransactionV1ConfigFailure::ExcessiveApprovals { .. } => None,
        }
    }
}

/// Error returned when a transaction is too large.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ExcessiveSizeErrorV1 {
    /// The maximum permitted serialized transaction size, in bytes.
    pub max_transaction_size: u32,
    /// The serialized size of the transaction provided, in bytes.
    pub actual_transaction_size: usize,
}

impl Display for ExcessiveSizeErrorV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction size of {} bytes exceeds limit of {}",
            self.actual_transaction_size, self.max_transaction_size
        )
    }
}

#[cfg(feature = "std")]
impl StdError for ExcessiveSizeErrorV1 {}

/// Errors other than validation failures relating to Transactions.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorV1 {
    /// Error while encoding to JSON.
    EncodeToJson(serde_json::Error),

    /// Error while decoding from JSON.
    DecodeFromJson(DecodeFromJsonErrorV1),
}

impl From<serde_json::Error> for ErrorV1 {
    fn from(error: serde_json::Error) -> Self {
        ErrorV1::EncodeToJson(error)
    }
}

impl From<DecodeFromJsonErrorV1> for ErrorV1 {
    fn from(error: DecodeFromJsonErrorV1) -> Self {
        ErrorV1::DecodeFromJson(error)
    }
}

impl Display for ErrorV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            ErrorV1::EncodeToJson(error) => {
                write!(formatter, "encoding to json: {}", error)
            }
            ErrorV1::DecodeFromJson(error) => {
                write!(formatter, "decoding from json: {}", error)
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for ErrorV1 {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            ErrorV1::EncodeToJson(error) => Some(error),
            ErrorV1::DecodeFromJson(error) => Some(error),
        }
    }
}

/// Error while decoding a `TransactionV1` from JSON.
#[derive(Debug)]
#[non_exhaustive]
pub enum DecodeFromJsonErrorV1 {
    /// Failed to decode from base 16.
    FromHex(base16::DecodeError),

    /// Failed to convert slice to array.
    TryFromSlice(TryFromSliceError),
}

impl From<base16::DecodeError> for DecodeFromJsonErrorV1 {
    fn from(error: base16::DecodeError) -> Self {
        DecodeFromJsonErrorV1::FromHex(error)
    }
}

impl From<TryFromSliceError> for DecodeFromJsonErrorV1 {
    fn from(error: TryFromSliceError) -> Self {
        DecodeFromJsonErrorV1::TryFromSlice(error)
    }
}

impl Display for DecodeFromJsonErrorV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            DecodeFromJsonErrorV1::FromHex(error) => {
                write!(formatter, "{}", error)
            }
            DecodeFromJsonErrorV1::TryFromSlice(error) => {
                write!(formatter, "{}", error)
            }
        }
    }
}

#[cfg(feature = "std")]
impl StdError for DecodeFromJsonErrorV1 {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            DecodeFromJsonErrorV1::FromHex(error) => Some(error),
            DecodeFromJsonErrorV1::TryFromSlice(error) => Some(error),
        }
    }
}
