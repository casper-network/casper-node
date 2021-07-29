//! Contains types and constants associated with user accounts.

mod account_hash;
pub mod action_thresholds;
mod action_type;
pub mod associated_keys;
mod error;
mod weight;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    contracts::NamedKeys,
    AccessRights, URef, BLAKE2B_DIGEST_LENGTH,
};
use alloc::{collections::BTreeSet, vec::Vec};
use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use core::{convert::TryFrom, fmt::Debug};
#[cfg(feature = "std")]
use thiserror::Error;

pub use self::{
    account_hash::{AccountHash, ACCOUNT_HASH_FORMATTED_STRING_PREFIX, ACCOUNT_HASH_LENGTH},
    action_thresholds::ActionThresholds,
    action_type::ActionType,
    associated_keys::AssociatedKeys,
    error::{FromStrError, SetThresholdFailure, TryFromIntError, TryFromSliceForAccountHashError},
    weight::{Weight, WEIGHT_SERIALIZED_LENGTH},
};

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Account {
    account_hash: AccountHash,
    named_keys: NamedKeys,
    main_purse: URef,
    associated_keys: AssociatedKeys,
    action_thresholds: ActionThresholds,
}

impl Account {
    pub fn new(
        account_hash: AccountHash,
        named_keys: NamedKeys,
        main_purse: URef,
        associated_keys: AssociatedKeys,
        action_thresholds: ActionThresholds,
    ) -> Self {
        Account {
            account_hash,
            named_keys,
            main_purse,
            associated_keys,
            action_thresholds,
        }
    }

    pub fn create(account: AccountHash, named_keys: NamedKeys, main_purse: URef) -> Self {
        let associated_keys = AssociatedKeys::new(account, Weight::new(1));
        let action_thresholds: ActionThresholds = Default::default();
        Account::new(
            account,
            named_keys,
            main_purse,
            associated_keys,
            action_thresholds,
        )
    }

    pub fn named_keys_append(&mut self, keys: &mut NamedKeys) {
        self.named_keys.append(keys);
    }

    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }

    pub fn named_keys_mut(&mut self) -> &mut NamedKeys {
        &mut self.named_keys
    }

    pub fn account_hash(&self) -> AccountHash {
        self.account_hash
    }

    pub fn main_purse(&self) -> URef {
        self.main_purse
    }

    /// Returns an [`AccessRights::ADD`]-only version of the [`URef`].
    pub fn main_purse_add_only(&self) -> URef {
        URef::new(self.main_purse.addr(), AccessRights::ADD)
    }

    pub fn associated_keys(&self) -> impl Iterator<Item = (&AccountHash, &Weight)> {
        self.associated_keys.iter()
    }

    pub fn action_thresholds(&self) -> &ActionThresholds {
        &self.action_thresholds
    }

    pub fn add_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), AddKeyFailure> {
        self.associated_keys.add_key(account_hash, weight)
    }

    /// Checks if removing given key would properly satisfy thresholds.
    fn can_remove_key(&self, account_hash: AccountHash) -> bool {
        let total_weight_without = self
            .associated_keys
            .total_keys_weight_excluding(account_hash);

        // Returns true if the total weight calculated without given public key would be greater or
        // equal to all of the thresholds.
        total_weight_without >= *self.action_thresholds().deployment()
            && total_weight_without >= *self.action_thresholds().key_management()
    }

    /// Checks if adding a weight to a sum of all weights excluding the given key would make the
    /// resulting value to fall below any of the thresholds on account.
    fn can_update_key(&self, account_hash: AccountHash, weight: Weight) -> bool {
        // Calculates total weight of all keys excluding the given key
        let total_weight = self
            .associated_keys
            .total_keys_weight_excluding(account_hash);

        // Safely calculate new weight by adding the updated weight
        let new_weight = total_weight.value().saturating_add(weight.value());

        // Returns true if the new weight would be greater or equal to all of
        // the thresholds.
        new_weight >= self.action_thresholds().deployment().value()
            && new_weight >= self.action_thresholds().key_management().value()
    }

    pub fn remove_associated_key(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<(), RemoveKeyFailure> {
        if self.associated_keys.contains_key(&account_hash) {
            // Check if removing this weight would fall below thresholds
            if !self.can_remove_key(account_hash) {
                return Err(RemoveKeyFailure::ThresholdViolation);
            }
        }
        self.associated_keys.remove_key(&account_hash)
    }

    pub fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), UpdateKeyFailure> {
        if let Some(current_weight) = self.associated_keys.get(&account_hash) {
            if weight < *current_weight {
                // New weight is smaller than current weight
                if !self.can_update_key(account_hash, weight) {
                    return Err(UpdateKeyFailure::ThresholdViolation);
                }
            }
        }
        self.associated_keys.update_key(account_hash, weight)
    }

    pub fn get_associated_key_weight(&self, account_hash: AccountHash) -> Option<&Weight> {
        self.associated_keys.get(&account_hash)
    }

    pub fn set_action_threshold(
        &mut self,
        action_type: ActionType,
        weight: Weight,
    ) -> Result<(), SetThresholdFailure> {
        // Verify if new threshold weight exceeds total weight of all associated
        // keys.
        self.can_set_threshold(weight)?;
        // Set new weight for given action
        self.action_thresholds.set_threshold(action_type, weight)
    }

    /// Verifies if user can set action threshold
    pub fn can_set_threshold(&self, new_threshold: Weight) -> Result<(), SetThresholdFailure> {
        let total_weight = self.associated_keys.total_keys_weight();
        if new_threshold > total_weight {
            return Err(SetThresholdFailure::InsufficientTotalWeight);
        }
        Ok(())
    }

    /// Checks whether all authorization keys are associated with this account
    pub fn can_authorize(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        !authorization_keys.is_empty()
            && authorization_keys
                .iter()
                .all(|e| self.associated_keys.contains_key(e))
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to deploy threshold.
    pub fn can_deploy_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        total_weight >= *self.action_thresholds().deployment()
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to key management threshold.
    pub fn can_manage_keys_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        total_weight >= *self.action_thresholds().key_management()
    }
}

impl ToBytes for Account {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.account_hash.to_bytes()?);
        result.append(&mut self.named_keys.to_bytes()?);
        result.append(&mut self.main_purse.to_bytes()?);
        result.append(&mut self.associated_keys.to_bytes()?);
        result.append(&mut self.action_thresholds.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.account_hash.serialized_length()
            + self.named_keys.serialized_length()
            + self.main_purse.serialized_length()
            + self.associated_keys.serialized_length()
            + self.action_thresholds.serialized_length()
    }
}

impl FromBytes for Account {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (account_hash, rem) = AccountHash::from_bytes(bytes)?;
        let (named_keys, rem) = NamedKeys::from_bytes(rem)?;
        let (main_purse, rem) = URef::from_bytes(rem)?;
        let (associated_keys, rem) = AssociatedKeys::from_bytes(rem)?;
        let (action_thresholds, rem) = ActionThresholds::from_bytes(rem)?;
        Ok((
            Account {
                account_hash,
                named_keys,
                main_purse,
                associated_keys,
                action_thresholds,
            },
            rem,
        ))
    }
}
/// Maximum number of associated keys (i.e. map of [`AccountHash`]s to [`Weight`]s) for a single
/// account.
pub const MAX_ASSOCIATED_KEYS: usize = 10;

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
