#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use casper_contract::{
    contract_api::{account, runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionType, NamedKeys, Weight},
    CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key, PackageHash, Parameter,
};

const ARG_ENTITY_ACCOUNT_HASH: &str = "entity_account_hash";
const ARG_KEY_WEIGHT: &str = "key_weight";
const ARG_NEW_UPGRADE_THRESHOLD: &str = "new_threshold";
const ARG_CONTRACT_PACKAGE: &str = "contract_package_hash";

const ENTRYPOINT_ADD_ASSOCIATED_KEY: &str = "add_associated_key";
const ENTRYPOINT_MANAGE_ACTION_THRESHOLD: &str = "manage_action_threshold";
const ENTRYPOINT_REMOVE_ASSOCIATED_KEY: &str = "remove_associated_key";

const CONTRACT_HASH_NAME: &str = "contract_hash_name";

#[no_mangle]
pub extern "C" fn add_associated_key() {
    let entity_account_hash: AccountHash = runtime::get_named_arg(ARG_ENTITY_ACCOUNT_HASH);
    let weight: u8 = runtime::get_named_arg(ARG_KEY_WEIGHT);
    account::add_associated_key(entity_account_hash, Weight::new(weight)).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn manage_action_threshold() {
    let new_threshold = runtime::get_named_arg(ARG_NEW_UPGRADE_THRESHOLD);
    account::set_action_threshold(ActionType::UpgradeManagement, new_threshold).unwrap_or_revert()
}

#[no_mangle]
pub extern "C" fn remove_associated_key() {
    let entity_account_hash: AccountHash = runtime::get_named_arg(ARG_ENTITY_ACCOUNT_HASH);
    account::remove_associated_key(entity_account_hash).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call() {
    let contract_package: PackageHash = runtime::get_named_arg(ARG_CONTRACT_PACKAGE);
    let entry_points = {
        let mut entrypoints = EntryPoints::new();
        let add_associated_key_entry_point = EntryPoint::new(
            ENTRYPOINT_ADD_ASSOCIATED_KEY,
            vec![
                Parameter::new(ARG_ENTITY_ACCOUNT_HASH, CLType::ByteArray(32)),
                Parameter::new(ARG_KEY_WEIGHT, CLType::U8),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entrypoints.add_entry_point(add_associated_key_entry_point);
        let manage_action_threshold_entrypoint = EntryPoint::new(
            ENTRYPOINT_MANAGE_ACTION_THRESHOLD,
            vec![Parameter::new(ARG_NEW_UPGRADE_THRESHOLD, CLType::U8)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entrypoints.add_entry_point(manage_action_threshold_entrypoint);
        let remove_associated_key_entry_point = EntryPoint::new(
            ENTRYPOINT_REMOVE_ASSOCIATED_KEY,
            vec![Parameter::new(
                ARG_ENTITY_ACCOUNT_HASH,
                CLType::ByteArray(32),
            )],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entrypoints.add_entry_point(remove_associated_key_entry_point);
        entrypoints
    };
    // this should overwrite the previous contract obj with the new contract obj at the same uref
    let (new_contract_hash, _new_contract_version) =
        storage::add_contract_version(contract_package, entry_points, NamedKeys::new());
    runtime::put_key(
        CONTRACT_HASH_NAME,
        Key::contract_entity_key(new_contract_hash),
    );
}
