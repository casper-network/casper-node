//! Contains types and constants associated with user accounts.

mod account_hash;
pub mod action_thresholds;
mod action_type;
pub mod associated_keys;
mod error;
mod weight;

use serde::Serialize;

use alloc::{collections::BTreeSet, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
    iter,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;

pub use self::{
    account_hash::{AccountHash, ACCOUNT_HASH_FORMATTED_STRING_PREFIX, ACCOUNT_HASH_LENGTH},
    action_thresholds::ActionThresholds,
    action_type::ActionType,
    associated_keys::AssociatedKeys,
    error::{FromStrError, SetThresholdFailure, TryFromIntError, TryFromSliceForAccountHashError},
    weight::{Weight, WEIGHT_SERIALIZED_LENGTH},
};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    contracts::NamedKeys,
    crypto, AccessRights, ContextAccessRights, Key, URef, BLAKE2B_DIGEST_LENGTH,
};

/// Represents an Account in the global state.
#[derive(PartialEq, Eq, Clone, Debug, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct Account {
    account_hash: AccountHash,
    named_keys: NamedKeys,
    main_purse: URef,
    associated_keys: AssociatedKeys,
    action_thresholds: ActionThresholds,
}

impl Account {
    /// Creates a new account.
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

    /// An Account constructor with presets for associated_keys and action_thresholds.
    ///
    /// An account created with this method is valid and can be used as the target of a transaction.
    /// It will be created with an [`AssociatedKeys`] with a [`Weight`] of 1, and a default
    /// [`ActionThresholds`].
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

    /// Extracts the access rights from the named keys and main purse of the account.
    pub fn extract_access_rights(&self) -> ContextAccessRights {
        let urefs_iter = self
            .named_keys
            .values()
            .filter_map(|key| key.as_uref().copied())
            .chain(iter::once(self.main_purse));
        ContextAccessRights::new(Key::from(self.account_hash), urefs_iter)
    }

    /// Appends named keys to an account's named_keys field.
    pub fn named_keys_append(&mut self, keys: &mut NamedKeys) {
        self.named_keys.append(keys);
    }

    /// Returns named keys.
    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }

    /// Returns a mutable reference to named keys.
    pub fn named_keys_mut(&mut self) -> &mut NamedKeys {
        &mut self.named_keys
    }

    /// Returns account hash.
    pub fn account_hash(&self) -> AccountHash {
        self.account_hash
    }

    /// Returns main purse.
    pub fn main_purse(&self) -> URef {
        self.main_purse
    }

    /// Returns an [`AccessRights::ADD`]-only version of the main purse's [`URef`].
    pub fn main_purse_add_only(&self) -> URef {
        URef::new(self.main_purse.addr(), AccessRights::ADD)
    }

    /// Returns associated keys.
    pub fn associated_keys(&self) -> &AssociatedKeys {
        &self.associated_keys
    }

    /// Returns action thresholds.
    pub fn action_thresholds(&self) -> &ActionThresholds {
        &self.action_thresholds
    }

    /// Adds an associated key to an account.
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

    /// Removes an associated key from an account.
    ///
    /// Verifies that removing the key will not cause the remaining weight to fall below any action
    /// thresholds.
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

    /// Updates an associated key.
    ///
    /// Returns an error if the update would result in a violation of the key management thresholds.
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

    /// Sets a new action threshold for a given action type for the account.
    ///
    /// Returns an error if the new action threshold weight is greater than the total weight of the
    /// account's associated keys.
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

    /// Verifies if user can set action threshold.
    pub fn can_set_threshold(&self, new_threshold: Weight) -> Result<(), SetThresholdFailure> {
        let total_weight = self.associated_keys.total_keys_weight();
        if new_threshold > total_weight {
            return Err(SetThresholdFailure::InsufficientTotalWeight);
        }
        Ok(())
    }

    /// Sets a new action threshold for a given action type for the account without checking against
    /// the total weight of the associated keys.
    ///
    /// This should only be called when authorized by an administrator account.
    ///
    /// Returns an error if setting the action would cause the `ActionType::Deployment` threshold to
    /// be greater than any of the other action types.
    pub fn set_action_threshold_unchecked(
        &mut self,
        action_type: ActionType,
        threshold: Weight,
    ) -> Result<(), SetThresholdFailure> {
        self.action_thresholds.set_threshold(action_type, threshold)
    }

    /// Checks whether all authorization keys are associated with this account.
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
        self.account_hash().write_bytes(&mut result)?;
        self.named_keys().write_bytes(&mut result)?;
        self.main_purse.write_bytes(&mut result)?;
        self.associated_keys().write_bytes(&mut result)?;
        self.action_thresholds().write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.account_hash.serialized_length()
            + self.named_keys.serialized_length()
            + self.main_purse.serialized_length()
            + self.associated_keys.serialized_length()
            + self.action_thresholds.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.account_hash().write_bytes(writer)?;
        self.named_keys().write_bytes(writer)?;
        self.main_purse().write_bytes(writer)?;
        self.associated_keys().write_bytes(writer)?;
        self.action_thresholds().write_bytes(writer)?;
        Ok(())
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

#[doc(hidden)]
#[deprecated(
    since = "1.4.4",
    note = "function moved to casper_types::crypto::blake2b"
)]
pub fn blake2b<T: AsRef<[u8]>>(data: T) -> [u8; BLAKE2B_DIGEST_LENGTH] {
    crypto::blake2b(data)
}

/// Errors that can occur while adding a new [`AccountHash`] to an account's associated keys map.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[repr(i32)]
#[non_exhaustive]
pub enum AddKeyFailure {
    /// There are already maximum [`AccountHash`]s associated with the given account.
    MaxKeysLimit = 1,
    /// The given [`AccountHash`] is already associated with the given account.
    DuplicateKey = 2,
    /// Caller doesn't have sufficient permissions to associate a new [`AccountHash`] with the
    /// given account.
    PermissionDenied = 3,
}

impl Display for AddKeyFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            AddKeyFailure::MaxKeysLimit => formatter.write_str(
                "Unable to add new associated key because maximum amount of keys is reached",
            ),
            AddKeyFailure::DuplicateKey => formatter
                .write_str("Unable to add new associated key because given key already exists"),
            AddKeyFailure::PermissionDenied => formatter
                .write_str("Unable to add new associated key due to insufficient permissions"),
        }
    }
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
#[repr(i32)]
#[non_exhaustive]
pub enum RemoveKeyFailure {
    /// The given [`AccountHash`] is not associated with the given account.
    MissingKey = 1,
    /// Caller doesn't have sufficient permissions to remove an associated [`AccountHash`] from the
    /// given account.
    PermissionDenied = 2,
    /// Removing the given associated [`AccountHash`] would cause the total weight of all remaining
    /// `AccountHash`s to fall below one of the action thresholds for the given account.
    ThresholdViolation = 3,
}

impl Display for RemoveKeyFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            RemoveKeyFailure::MissingKey => {
                formatter.write_str("Unable to remove a key that does not exist")
            }
            RemoveKeyFailure::PermissionDenied => formatter
                .write_str("Unable to remove associated key due to insufficient permissions"),
            RemoveKeyFailure::ThresholdViolation => formatter.write_str(
                "Unable to remove a key which would violate action threshold constraints",
            ),
        }
    }
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
#[repr(i32)]
#[non_exhaustive]
pub enum UpdateKeyFailure {
    /// The given [`AccountHash`] is not associated with the given account.
    MissingKey = 1,
    /// Caller doesn't have sufficient permissions to update an associated [`AccountHash`] from the
    /// given account.
    PermissionDenied = 2,
    /// Updating the [`Weight`] of the given associated [`AccountHash`] would cause the total
    /// weight of all `AccountHash`s to fall below one of the action thresholds for the given
    /// account.
    ThresholdViolation = 3,
}

impl Display for UpdateKeyFailure {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            UpdateKeyFailure::MissingKey => formatter.write_str(
                "Unable to update the value under an associated key that does not exist",
            ),
            UpdateKeyFailure::PermissionDenied => formatter
                .write_str("Unable to update associated key due to insufficient permissions"),
            UpdateKeyFailure::ThresholdViolation => formatter.write_str(
                "Unable to update weight that would fall below any of action thresholds",
            ),
        }
    }
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

#[doc(hidden)]
#[cfg(any(feature = "testing", feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use crate::{
        account::{
            action_thresholds::gens::action_thresholds_arb,
            associated_keys::gens::associated_keys_arb, Account, Weight,
        },
        gens::{account_hash_arb, named_keys_arb, uref_arb},
    };

    prop_compose! {
        pub fn account_arb()(
            account_hash in account_hash_arb(),
            urefs in named_keys_arb(3),
            purse in uref_arb(),
            thresholds in action_thresholds_arb(),
            mut associated_keys in associated_keys_arb(),
        ) -> Account {
                associated_keys.add_key(account_hash, Weight::new(1)).unwrap();
                Account::new(
                    account_hash,
                    urefs,
                    purse,
                    associated_keys,
                    thresholds,
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        account::{
            Account, AccountHash, ActionThresholds, ActionType, AssociatedKeys, RemoveKeyFailure,
            SetThresholdFailure, UpdateKeyFailure, Weight,
        },
        contracts::NamedKeys,
        AccessRights, URef,
    };
    use std::{collections::BTreeSet, convert::TryFrom, iter::FromIterator, vec::Vec};

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

    #[test]
    fn associated_keys_can_authorize_keys() {
        let key_1 = AccountHash::new([0; 32]);
        let key_2 = AccountHash::new([1; 32]);
        let key_3 = AccountHash::new([2; 32]);
        let mut keys = AssociatedKeys::default();

        keys.add_key(key_2, Weight::new(2))
            .expect("should add key_1");
        keys.add_key(key_1, Weight::new(1))
            .expect("should add key_1");
        keys.add_key(key_3, Weight::new(3))
            .expect("should add key_1");

        let account = Account::new(
            AccountHash::new([0u8; 32]),
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            keys,
            // deploy: 33 (3*11)
            ActionThresholds::new(Weight::new(33), Weight::new(48))
                .expect("should create thresholds"),
        );

        assert!(account.can_authorize(&BTreeSet::from_iter(vec![key_3, key_2, key_1])));
        assert!(account.can_authorize(&BTreeSet::from_iter(vec![key_1, key_3, key_2])));

        assert!(account.can_authorize(&BTreeSet::from_iter(vec![key_1, key_2])));
        assert!(account.can_authorize(&BTreeSet::from_iter(vec![key_1])));

        assert!(!account.can_authorize(&BTreeSet::from_iter(vec![
            key_1,
            key_2,
            AccountHash::new([42; 32])
        ])));
        assert!(!account.can_authorize(&BTreeSet::from_iter(vec![
            AccountHash::new([42; 32]),
            key_1,
            key_2
        ])));
        assert!(!account.can_authorize(&BTreeSet::from_iter(vec![
            AccountHash::new([43; 32]),
            AccountHash::new([44; 32]),
            AccountHash::new([42; 32])
        ])));
        assert!(!account.can_authorize(&BTreeSet::new()));
    }

    #[test]
    fn account_can_deploy_with() {
        let associated_keys = {
            let mut res = AssociatedKeys::new(AccountHash::new([1u8; 32]), Weight::new(1));
            res.add_key(AccountHash::new([2u8; 32]), Weight::new(11))
                .expect("should add key 1");
            res.add_key(AccountHash::new([3u8; 32]), Weight::new(11))
                .expect("should add key 2");
            res.add_key(AccountHash::new([4u8; 32]), Weight::new(11))
                .expect("should add key 3");
            res
        };
        let account = Account::new(
            AccountHash::new([0u8; 32]),
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            associated_keys,
            // deploy: 33 (3*11)
            ActionThresholds::new(Weight::new(33), Weight::new(48))
                .expect("should create thresholds"),
        );

        // sum: 22, required 33 - can't deploy
        assert!(!account.can_deploy_with(&BTreeSet::from_iter(vec![
            AccountHash::new([3u8; 32]),
            AccountHash::new([2u8; 32]),
        ])));

        // sum: 33, required 33 - can deploy
        assert!(account.can_deploy_with(&BTreeSet::from_iter(vec![
            AccountHash::new([4u8; 32]),
            AccountHash::new([3u8; 32]),
            AccountHash::new([2u8; 32]),
        ])));

        // sum: 34, required 33 - can deploy
        assert!(account.can_deploy_with(&BTreeSet::from_iter(vec![
            AccountHash::new([2u8; 32]),
            AccountHash::new([1u8; 32]),
            AccountHash::new([4u8; 32]),
            AccountHash::new([3u8; 32]),
        ])));
    }

    #[test]
    fn account_can_manage_keys_with() {
        let associated_keys = {
            let mut res = AssociatedKeys::new(AccountHash::new([1u8; 32]), Weight::new(1));
            res.add_key(AccountHash::new([2u8; 32]), Weight::new(11))
                .expect("should add key 1");
            res.add_key(AccountHash::new([3u8; 32]), Weight::new(11))
                .expect("should add key 2");
            res.add_key(AccountHash::new([4u8; 32]), Weight::new(11))
                .expect("should add key 3");
            res
        };
        let account = Account::new(
            AccountHash::new([0u8; 32]),
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            associated_keys,
            // deploy: 33 (3*11)
            ActionThresholds::new(Weight::new(11), Weight::new(33))
                .expect("should create thresholds"),
        );

        // sum: 22, required 33 - can't manage
        assert!(!account.can_manage_keys_with(&BTreeSet::from_iter(vec![
            AccountHash::new([3u8; 32]),
            AccountHash::new([2u8; 32]),
        ])));

        // sum: 33, required 33 - can manage
        assert!(account.can_manage_keys_with(&BTreeSet::from_iter(vec![
            AccountHash::new([4u8; 32]),
            AccountHash::new([3u8; 32]),
            AccountHash::new([2u8; 32]),
        ])));

        // sum: 34, required 33 - can manage
        assert!(account.can_manage_keys_with(&BTreeSet::from_iter(vec![
            AccountHash::new([2u8; 32]),
            AccountHash::new([1u8; 32]),
            AccountHash::new([4u8; 32]),
            AccountHash::new([3u8; 32]),
        ])));
    }

    #[test]
    fn set_action_threshold_higher_than_total_weight() {
        let identity_key = AccountHash::new([1u8; 32]);
        let key_1 = AccountHash::new([2u8; 32]);
        let key_2 = AccountHash::new([3u8; 32]);
        let key_3 = AccountHash::new([4u8; 32]);
        let associated_keys = {
            let mut res = AssociatedKeys::new(identity_key, Weight::new(1));
            res.add_key(key_1, Weight::new(2))
                .expect("should add key 1");
            res.add_key(key_2, Weight::new(3))
                .expect("should add key 2");
            res.add_key(key_3, Weight::new(4))
                .expect("should add key 3");
            res
        };
        let mut account = Account::new(
            AccountHash::new([0u8; 32]),
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            associated_keys,
            // deploy: 33 (3*11)
            ActionThresholds::new(Weight::new(33), Weight::new(48))
                .expect("should create thresholds"),
        );

        assert_eq!(
            account
                .set_action_threshold(ActionType::Deployment, Weight::new(1 + 2 + 3 + 4 + 1))
                .unwrap_err(),
            SetThresholdFailure::InsufficientTotalWeight,
        );
        assert_eq!(
            account
                .set_action_threshold(ActionType::Deployment, Weight::new(1 + 2 + 3 + 4 + 245))
                .unwrap_err(),
            SetThresholdFailure::InsufficientTotalWeight,
        )
    }

    #[test]
    fn remove_key_would_violate_action_thresholds() {
        let identity_key = AccountHash::new([1u8; 32]);
        let key_1 = AccountHash::new([2u8; 32]);
        let key_2 = AccountHash::new([3u8; 32]);
        let key_3 = AccountHash::new([4u8; 32]);
        let associated_keys = {
            let mut res = AssociatedKeys::new(identity_key, Weight::new(1));
            res.add_key(key_1, Weight::new(2))
                .expect("should add key 1");
            res.add_key(key_2, Weight::new(3))
                .expect("should add key 2");
            res.add_key(key_3, Weight::new(4))
                .expect("should add key 3");
            res
        };
        let mut account = Account::new(
            AccountHash::new([0u8; 32]),
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            associated_keys,
            // deploy: 33 (3*11)
            ActionThresholds::new(Weight::new(1 + 2 + 3 + 4), Weight::new(1 + 2 + 3 + 4 + 5))
                .expect("should create thresholds"),
        );

        assert_eq!(
            account.remove_associated_key(key_3).unwrap_err(),
            RemoveKeyFailure::ThresholdViolation,
        )
    }

    #[test]
    fn updating_key_would_violate_action_thresholds() {
        let identity_key = AccountHash::new([1u8; 32]);
        let identity_key_weight = Weight::new(1);
        let key_1 = AccountHash::new([2u8; 32]);
        let key_1_weight = Weight::new(2);
        let key_2 = AccountHash::new([3u8; 32]);
        let key_2_weight = Weight::new(3);
        let key_3 = AccountHash::new([4u8; 32]);
        let key_3_weight = Weight::new(4);
        let associated_keys = {
            let mut res = AssociatedKeys::new(identity_key, identity_key_weight);
            res.add_key(key_1, key_1_weight).expect("should add key 1");
            res.add_key(key_2, key_2_weight).expect("should add key 2");
            res.add_key(key_3, key_3_weight).expect("should add key 3");
            // 1 + 2 + 3 + 4
            res
        };

        let deployment_threshold = Weight::new(
            identity_key_weight.value()
                + key_1_weight.value()
                + key_2_weight.value()
                + key_3_weight.value(),
        );
        let key_management_threshold = Weight::new(deployment_threshold.value() + 1);
        let mut account = Account::new(
            identity_key,
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            associated_keys,
            // deploy: 33 (3*11)
            ActionThresholds::new(deployment_threshold, key_management_threshold)
                .expect("should create thresholds"),
        );

        // Decreases by 3
        assert_eq!(
            account
                .clone()
                .update_associated_key(key_3, Weight::new(1))
                .unwrap_err(),
            UpdateKeyFailure::ThresholdViolation,
        );

        // increase total weight (12)
        account
            .update_associated_key(identity_key, Weight::new(3))
            .unwrap();

        // variant a) decrease total weight by 1 (total 11)
        account
            .clone()
            .update_associated_key(key_3, Weight::new(3))
            .unwrap();
        // variant b) decrease total weight by 3 (total 9) - fail
        assert_eq!(
            account
                .update_associated_key(key_3, Weight::new(1))
                .unwrap_err(),
            UpdateKeyFailure::ThresholdViolation
        );
    }

    #[test]
    fn overflowing_should_allow_removal() {
        let identity_key = AccountHash::new([42; 32]);
        let key_1 = AccountHash::new([2u8; 32]);
        let key_2 = AccountHash::new([3u8; 32]);

        let associated_keys = {
            // Identity
            let mut res = AssociatedKeys::new(identity_key, Weight::new(1));

            // Spare key
            res.add_key(key_1, Weight::new(2))
                .expect("should add key 1");
            // Big key
            res.add_key(key_2, Weight::new(255))
                .expect("should add key 2");

            res
        };

        let mut account = Account::new(
            identity_key,
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            associated_keys,
            ActionThresholds::new(Weight::new(1), Weight::new(254))
                .expect("should create thresholds"),
        );

        account.remove_associated_key(key_1).expect("should work")
    }

    #[test]
    fn overflowing_should_allow_updating() {
        let identity_key = AccountHash::new([1; 32]);
        let identity_key_weight = Weight::new(1);
        let key_1 = AccountHash::new([2u8; 32]);
        let key_1_weight = Weight::new(3);
        let key_2 = AccountHash::new([3u8; 32]);
        let key_2_weight = Weight::new(255);
        let deployment_threshold = Weight::new(1);
        let key_management_threshold = Weight::new(254);

        let associated_keys = {
            // Identity
            let mut res = AssociatedKeys::new(identity_key, identity_key_weight);

            // Spare key
            res.add_key(key_1, key_1_weight).expect("should add key 1");
            // Big key
            res.add_key(key_2, key_2_weight).expect("should add key 2");

            res
        };

        let mut account = Account::new(
            identity_key,
            NamedKeys::new(),
            URef::new([0u8; 32], AccessRights::READ_ADD_WRITE),
            associated_keys,
            ActionThresholds::new(deployment_threshold, key_management_threshold)
                .expect("should create thresholds"),
        );

        // decrease so total weight would be changed from 1 + 3 + 255 to 1 + 1 + 255
        account
            .update_associated_key(key_1, Weight::new(1))
            .expect("should work");
    }

    #[test]
    fn should_extract_access_rights() {
        const MAIN_PURSE: URef = URef::new([2; 32], AccessRights::READ_ADD_WRITE);
        const OTHER_UREF: URef = URef::new([3; 32], AccessRights::READ);

        let account_hash = AccountHash::new([1u8; 32]);
        let mut named_keys = NamedKeys::new();
        named_keys.insert("a".to_string(), Key::URef(OTHER_UREF));
        let associated_keys = AssociatedKeys::new(account_hash, Weight::new(1));
        let account = Account::new(
            account_hash,
            named_keys,
            MAIN_PURSE,
            associated_keys,
            ActionThresholds::new(Weight::new(1), Weight::new(1))
                .expect("should create thresholds"),
        );

        let actual_access_rights = account.extract_access_rights();

        let expected_access_rights =
            ContextAccessRights::new(Key::from(account_hash), vec![MAIN_PURSE, OTHER_UREF]);
        assert_eq!(actual_access_rights, expected_access_rights)
    }
}

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use crate::bytesrepr;

    use super::*;

    proptest! {
        #[test]
        fn test_value_account(acct in gens::account_arb()) {
            bytesrepr::test_serialization_roundtrip(&acct);
        }
    }
}
