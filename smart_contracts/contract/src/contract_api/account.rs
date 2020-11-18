//! Functions for managing accounts.

use alloc::vec::Vec;
use core::convert::TryFrom;

use casper_types::{
    account::{
        AccountHash, ActionType, AddKeyFailure, RemoveKeyFailure, SetThresholdFailure,
        UpdateKeyFailure, Weight,
    },
    bytesrepr, URef, UREF_SERIALIZED_LENGTH,
};

use super::to_ptr;
use crate::{contract_api, ext_ffi, unwrap_or_revert::UnwrapOrRevert};

/// Retrieves the ID of the account's main purse.
pub fn get_main_purse() -> URef {
    let dest_non_null_ptr = contract_api::alloc_bytes(UREF_SERIALIZED_LENGTH);
    let bytes = unsafe {
        ext_ffi::casper_get_main_purse(dest_non_null_ptr.as_ptr());
        Vec::from_raw_parts(
            dest_non_null_ptr.as_ptr(),
            UREF_SERIALIZED_LENGTH,
            UREF_SERIALIZED_LENGTH,
        )
    };
    bytesrepr::deserialize(bytes).unwrap_or_revert()
}

/// Sets the given [`ActionType`]'s threshold to the provided value.
pub fn set_action_threshold(
    action_type: ActionType,
    threshold: Weight,
) -> Result<(), SetThresholdFailure> {
    let action_type = action_type as u32;
    let threshold = threshold.value().into();
    let result = unsafe { ext_ffi::casper_set_action_threshold(action_type, threshold) };
    if result == 0 {
        Ok(())
    } else {
        Err(SetThresholdFailure::try_from(result).unwrap_or_revert())
    }
}

/// Adds the given [`AccountHash`] with associated [`Weight`] to the account's associated keys.
pub fn add_associated_key(account_hash: AccountHash, weight: Weight) -> Result<(), AddKeyFailure> {
    let (account_hash_ptr, account_hash_size, _bytes) = to_ptr(account_hash);
    // Cast of u8 (weight) into i32 is assumed to be always safe
    let result = unsafe {
        ext_ffi::casper_add_associated_key(
            account_hash_ptr,
            account_hash_size,
            weight.value().into(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(AddKeyFailure::try_from(result).unwrap_or_revert())
    }
}

/// Removes the given [`AccountHash`] from the account's associated keys.
pub fn remove_associated_key(account_hash: AccountHash) -> Result<(), RemoveKeyFailure> {
    let (account_hash_ptr, account_hash_size, _bytes) = to_ptr(account_hash);
    let result =
        unsafe { ext_ffi::casper_remove_associated_key(account_hash_ptr, account_hash_size) };
    if result == 0 {
        Ok(())
    } else {
        Err(RemoveKeyFailure::try_from(result).unwrap_or_revert())
    }
}

/// Updates the [`Weight`] of the given [`AccountHash`] in the account's associated keys.
pub fn update_associated_key(
    account_hash: AccountHash,
    weight: Weight,
) -> Result<(), UpdateKeyFailure> {
    let (account_hash_ptr, account_hash_size, _bytes) = to_ptr(account_hash);
    // Cast of u8 (weight) into i32 is assumed to be always safe
    let result = unsafe {
        ext_ffi::casper_update_associated_key(
            account_hash_ptr,
            account_hash_size,
            weight.value().into(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(UpdateKeyFailure::try_from(result).unwrap_or_revert())
    }
}
