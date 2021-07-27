use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};
#[cfg(feature = "std")]
use thiserror::Error;

// This error type is not intended to be used by third party crates.
#[doc(hidden)]
#[derive(Debug, Eq, PartialEq)]
pub struct TryFromIntError(pub ());

/// Error returned when decoding an `AccountHash` from a formatted string.
#[derive(Debug)]
pub enum FromStrError {
    /// The prefix is invalid.
    InvalidPrefix,
    /// The hash is not valid hex.
    Hex(base16::DecodeError),
    /// The hash is the wrong length.
    Hash(TryFromSliceError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl From<TryFromSliceError> for FromStrError {
    fn from(error: TryFromSliceError) -> Self {
        FromStrError::Hash(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::InvalidPrefix => write!(f, "prefix is not 'account-hash-'"),
            FromStrError::Hex(error) => {
                write!(f, "failed to decode address portion from hex: {}", error)
            }
            FromStrError::Hash(error) => write!(f, "address portion is wrong length: {}", error),
        }
    }
}

/// Errors that can occur while changing action thresholds (i.e. the total
/// [`Weight`](super::Weight)s of signing [`AccountHash`](super::AccountHash)s required to perform
/// various actions) on an account.
#[repr(i32)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum SetThresholdFailure {
    /// Setting the key-management threshold to a value lower than the deployment threshold is
    /// disallowed.
    #[cfg_attr(
        feature = "std",
        error("New threshold should be greater than or equal to deployment threshold")
    )]
    KeyManagementThreshold = 1,
    /// Setting the deployment threshold to a value greater than any other threshold is disallowed.
    #[cfg_attr(
        feature = "std",
        error("New threshold should be lower than or equal to key management threshold")
    )]
    DeploymentThreshold = 2,
    /// Caller doesn't have sufficient permissions to set new thresholds.
    #[cfg_attr(
        feature = "std",
        error("Unable to set action threshold due to insufficient permissions")
    )]
    PermissionDeniedError = 3,
    /// Setting a threshold to a value greater than the total weight of associated keys is
    /// disallowed.
    #[cfg_attr(
        feature = "std",
        error("New threshold should be lower or equal than total weight of associated keys")
    )]
    InsufficientTotalWeight = 4,
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<i32> for SetThresholdFailure {
    type Error = TryFromIntError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            d if d == SetThresholdFailure::KeyManagementThreshold as i32 => {
                Ok(SetThresholdFailure::KeyManagementThreshold)
            }
            d if d == SetThresholdFailure::DeploymentThreshold as i32 => {
                Ok(SetThresholdFailure::DeploymentThreshold)
            }
            d if d == SetThresholdFailure::PermissionDeniedError as i32 => {
                Ok(SetThresholdFailure::PermissionDeniedError)
            }
            d if d == SetThresholdFailure::InsufficientTotalWeight as i32 => {
                Ok(SetThresholdFailure::InsufficientTotalWeight)
            }
            _ => Err(TryFromIntError(())),
        }
    }
}
