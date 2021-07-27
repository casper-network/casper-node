//! Contains types and constants associated with user accounts.

mod account_hash;

use crate::{
    bytesrepr::{Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped, PublicKey, BLAKE2B_DIGEST_LENGTH,
};
use alloc::vec::Vec;
use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "std")]
use thiserror::Error;

pub use self::account_hash::{
    AccountHash, AccountHashBytes, ACCOUNT_HASH_FORMATTED_STRING_PREFIX, ACCOUNT_HASH_LENGTH,
};

// This error type is not intended to be used by third party crates.
#[doc(hidden)]
#[derive(Debug, Eq, PartialEq)]
pub struct TryFromIntError(());

/// Associated error type of `TryFrom<&[u8]>` for [`AccountHash`].
#[derive(Debug)]
pub struct TryFromSliceForAccountHashError(());

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

/// The various types of action which can be performed in the context of a given account.
#[repr(u32)]
pub enum ActionType {
    /// Represents performing a deploy.
    Deployment = 0,
    /// Represents changing the associated keys (i.e. map of [`AccountHash`]s to [`Weight`]s) or
    /// action thresholds (i.e. the total [`Weight`]s of signing [`AccountHash`]s required to
    /// perform various actions).
    KeyManagement = 1,
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<u32> for ActionType {
    type Error = TryFromIntError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        // This doesn't use `num_derive` traits such as FromPrimitive and ToPrimitive
        // that helps to automatically create `from_u32` and `to_u32`. This approach
        // gives better control over generated code.
        match value {
            d if d == ActionType::Deployment as u32 => Ok(ActionType::Deployment),
            d if d == ActionType::KeyManagement as u32 => Ok(ActionType::KeyManagement),
            _ => Err(TryFromIntError(())),
        }
    }
}

/// Errors that can occur while changing action thresholds (i.e. the total [`Weight`]s of signing
/// [`AccountHash`]s required to perform various actions) on an account.
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

/// Maximum number of associated keys (i.e. map of [`AccountHash`]s to [`Weight`]s) for a single
/// account.
pub const MAX_ASSOCIATED_KEYS: usize = 10;

/// The number of bytes in a serialized [`Weight`].
pub const WEIGHT_SERIALIZED_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// The weight attributed to a given [`AccountHash`] in an account's associated keys.
#[derive(PartialOrd, Ord, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Weight(u8);

impl Weight {
    /// Constructs a new `Weight`.
    pub fn new(weight: u8) -> Weight {
        Weight(weight)
    }

    /// Returns the value of `self` as a `u8`.
    pub fn value(self) -> u8 {
        self.0
    }
}

impl ToBytes for Weight {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        WEIGHT_SERIALIZED_LENGTH
    }
}

impl FromBytes for Weight {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (byte, rem) = u8::from_bytes(bytes)?;
        Ok((Weight::new(byte), rem))
    }
}

impl CLTyped for Weight {
    fn cl_type() -> CLType {
        CLType::U8
    }
}

#[doc(hidden)]
pub fn blake2b<T: AsRef<[u8]>>(data: T) -> [u8; BLAKE2B_DIGEST_LENGTH] {
    let mut result = [0; BLAKE2B_DIGEST_LENGTH];
    // NOTE: Assumed safe as `BLAKE2B_DIGEST_LENGTH` is a valid value for a hasher
    let mut hasher = VarBlake2b::new(BLAKE2B_DIGEST_LENGTH).expect("should create hasher");

    hasher.update(data);
    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    result
}

impl TryFrom<&[u8]> for AccountHash {
    type Error = TryFromSliceForAccountHashError;

    fn try_from(bytes: &[u8]) -> Result<Self, TryFromSliceForAccountHashError> {
        AccountHashBytes::try_from(bytes)
            .map(AccountHash::new)
            .map_err(|_| TryFromSliceForAccountHashError(()))
    }
}

impl TryFrom<&alloc::vec::Vec<u8>> for AccountHash {
    type Error = TryFromSliceForAccountHashError;

    fn try_from(bytes: &Vec<u8>) -> Result<Self, Self::Error> {
        AccountHashBytes::try_from(bytes as &[u8])
            .map(AccountHash::new)
            .map_err(|_| TryFromSliceForAccountHashError(()))
    }
}

impl From<&PublicKey> for AccountHash {
    fn from(public_key: &PublicKey) -> Self {
        AccountHash::from_public_key(public_key, blake2b)
    }
}

impl Display for AccountHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for AccountHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "AccountHash({})", base16::encode_lower(&self.0))
    }
}

impl CLTyped for AccountHash {
    fn cl_type() -> CLType {
        CLType::ByteArray(ACCOUNT_HASH_LENGTH as u32)
    }
}

impl ToBytes for AccountHash {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for AccountHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((AccountHash::new(bytes), rem))
    }
}

impl AsRef<[u8]> for AccountHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Distribution<AccountHash> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AccountHash {
        AccountHash::new(rng.gen())
    }
}

/// Errors that can occur while adding a new [`AccountHash`] to an account's associated keys map.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(Error))]
#[repr(i32)]
pub enum AddKeyFailure {
    /// There are already [`MAX_ASSOCIATED_KEYS`] [`AccountHash`]s associated with the given
    /// account.
    #[cfg_attr(
        feature = "std",
        error("Unable to add new associated key because maximum amount of keys is reached")
    )]
    MaxKeysLimit = 1,
    /// The given [`AccountHash`] is already associated with the given account.
    #[cfg_attr(
        feature = "std",
        error("Unable to add new associated key because given key already exists")
    )]
    DuplicateKey = 2,
    /// Caller doesn't have sufficient permissions to associate a new [`AccountHash`] with the
    /// given account.
    #[cfg_attr(
        feature = "std",
        error("Unable to add new associated key due to insufficient permissions")
    )]
    PermissionDenied = 3,
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<i32> for AddKeyFailure {
    type Error = TryFromIntError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            d if d == AddKeyFailure::MaxKeysLimit as i32 => Ok(AddKeyFailure::MaxKeysLimit),
            d if d == AddKeyFailure::DuplicateKey as i32 => Ok(AddKeyFailure::DuplicateKey),
            d if d == AddKeyFailure::PermissionDenied as i32 => Ok(AddKeyFailure::PermissionDenied),
            _ => Err(TryFromIntError(())),
        }
    }
}

/// Errors that can occur while removing a [`AccountHash`] from an account's associated keys map.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[cfg_attr(feature = "std", derive(Error))]
#[repr(i32)]
pub enum RemoveKeyFailure {
    /// The given [`AccountHash`] is not associated with the given account.
    #[cfg_attr(feature = "std", error("Unable to remove a key that does not exist"))]
    MissingKey = 1,
    /// Caller doesn't have sufficient permissions to remove an associated [`AccountHash`] from the
    /// given account.
    #[cfg_attr(
        feature = "std",
        error("Unable to remove associated key due to insufficient permissions")
    )]
    PermissionDenied = 2,
    /// Removing the given associated [`AccountHash`] would cause the total weight of all remaining
    /// `AccountHash`s to fall below one of the action thresholds for the given account.
    #[cfg_attr(
        feature = "std",
        error("Unable to remove a key which would violate action threshold constraints")
    )]
    ThresholdViolation = 3,
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<i32> for RemoveKeyFailure {
    type Error = TryFromIntError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            d if d == RemoveKeyFailure::MissingKey as i32 => Ok(RemoveKeyFailure::MissingKey),
            d if d == RemoveKeyFailure::PermissionDenied as i32 => {
                Ok(RemoveKeyFailure::PermissionDenied)
            }
            d if d == RemoveKeyFailure::ThresholdViolation as i32 => {
                Ok(RemoveKeyFailure::ThresholdViolation)
            }
            _ => Err(TryFromIntError(())),
        }
    }
}

/// Errors that can occur while updating the [`Weight`] of a [`AccountHash`] in an account's
/// associated keys map.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "std", derive(Error))]
#[repr(i32)]
pub enum UpdateKeyFailure {
    /// The given [`AccountHash`] is not associated with the given account.
    #[cfg_attr(
        feature = "std",
        error("Unable to update the value under an associated key that does not exist")
    )]
    MissingKey = 1,
    /// Caller doesn't have sufficient permissions to update an associated [`AccountHash`] from the
    /// given account.
    #[cfg_attr(
        feature = "std",
        error("Unable to update associated key due to insufficient permissions")
    )]
    PermissionDenied = 2,
    /// Updating the [`Weight`] of the given associated [`AccountHash`] would cause the total
    /// weight of all `AccountHash`s to fall below one of the action thresholds for the given
    /// account.
    #[cfg_attr(
        feature = "std",
        error("Unable to update weight that would fall below any of action thresholds")
    )]
    ThresholdViolation = 3,
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<i32> for UpdateKeyFailure {
    type Error = TryFromIntError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            d if d == UpdateKeyFailure::MissingKey as i32 => Ok(UpdateKeyFailure::MissingKey),
            d if d == UpdateKeyFailure::PermissionDenied as i32 => {
                Ok(UpdateKeyFailure::PermissionDenied)
            }
            d if d == UpdateKeyFailure::ThresholdViolation as i32 => {
                Ok(UpdateKeyFailure::ThresholdViolation)
            }
            _ => Err(TryFromIntError(())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::TryFrom, vec::Vec};

    use super::*;

    #[test]
    fn account_hash_from_slice() {
        let bytes: Vec<u8> = (0..32).collect();
        let account_hash = AccountHash::try_from(&bytes[..]).expect("should create account hash");
        assert_eq!(&bytes, &account_hash.as_bytes());
    }

    #[test]
    fn account_hash_from_slice_too_small() {
        let _account_hash =
            AccountHash::try_from(&[0u8; 31][..]).expect_err("should not create account hash");
    }

    #[test]
    fn account_hash_from_slice_too_big() {
        let _account_hash =
            AccountHash::try_from(&[0u8; 33][..]).expect_err("should not create account hash");
    }

    #[test]
    fn try_from_i32_for_set_threshold_failure() {
        let max_valid_value_for_variant = SetThresholdFailure::InsufficientTotalWeight as i32;
        assert_eq!(
            Err(TryFromIntError(())),
            SetThresholdFailure::try_from(max_valid_value_for_variant + 1),
            "Did you forget to update `SetThresholdFailure::try_from` for a new variant of \
                   `SetThresholdFailure`, or `max_valid_value_for_variant` in this test?"
        );
    }

    #[test]
    fn try_from_i32_for_add_key_failure() {
        let max_valid_value_for_variant = AddKeyFailure::PermissionDenied as i32;
        assert_eq!(
            Err(TryFromIntError(())),
            AddKeyFailure::try_from(max_valid_value_for_variant + 1),
            "Did you forget to update `AddKeyFailure::try_from` for a new variant of \
                   `AddKeyFailure`, or `max_valid_value_for_variant` in this test?"
        );
    }

    #[test]
    fn try_from_i32_for_remove_key_failure() {
        let max_valid_value_for_variant = RemoveKeyFailure::ThresholdViolation as i32;
        assert_eq!(
            Err(TryFromIntError(())),
            RemoveKeyFailure::try_from(max_valid_value_for_variant + 1),
            "Did you forget to update `RemoveKeyFailure::try_from` for a new variant of \
                   `RemoveKeyFailure`, or `max_valid_value_for_variant` in this test?"
        );
    }

    #[test]
    fn try_from_i32_for_update_key_failure() {
        let max_valid_value_for_variant = UpdateKeyFailure::ThresholdViolation as i32;
        assert_eq!(
            Err(TryFromIntError(())),
            UpdateKeyFailure::try_from(max_valid_value_for_variant + 1),
            "Did you forget to update `UpdateKeyFailure::try_from` for a new variant of \
                   `UpdateKeyFailure`, or `max_valid_value_for_variant` in this test?"
        );
    }

    #[test]
    fn account_hash_from_str() {
        let account_hash = AccountHash([3; 32]);
        let encoded = account_hash.to_formatted_string();
        let decoded = AccountHash::from_formatted_str(&encoded).unwrap();
        assert_eq!(account_hash, decoded);

        let invalid_prefix =
            "accounthash-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(AccountHash::from_formatted_str(invalid_prefix).is_err());

        let invalid_prefix =
            "account-hash0000000000000000000000000000000000000000000000000000000000000000";
        assert!(AccountHash::from_formatted_str(invalid_prefix).is_err());

        let short_addr =
            "account-hash-00000000000000000000000000000000000000000000000000000000000000";
        assert!(AccountHash::from_formatted_str(short_addr).is_err());

        let long_addr =
            "account-hash-000000000000000000000000000000000000000000000000000000000000000000";
        assert!(AccountHash::from_formatted_str(long_addr).is_err());

        let invalid_hex =
            "account-hash-000000000000000000000000000000000000000000000000000000000000000g";
        assert!(AccountHash::from_formatted_str(invalid_hex).is_err());
    }

    #[test]
    fn account_hash_serde_roundtrip() {
        let account_hash = AccountHash([255; 32]);
        let serialized = bincode::serialize(&account_hash).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(account_hash, decoded);
    }

    #[test]
    fn account_hash_json_roundtrip() {
        let account_hash = AccountHash([255; 32]);
        let json_string = serde_json::to_string_pretty(&account_hash).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(account_hash, decoded);
    }
}
