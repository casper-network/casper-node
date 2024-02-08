#![no_std]
#![no_main]

extern crate alloc;

use alloc::{collections::BTreeMap, string::ToString, vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};

use casper_types::{
    account::AccountHash,
    addressable_entity::{
        EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys, Parameter,
    },
    CLType, CLTyped, Key, U512,
};

const ENTRY_FUNCTION_NAME: &str = "transfer";

const PACKAGE_HASH_KEY_NAME: &str = "transfer_purse_to_accounts";
const HASH_KEY_NAME: &str = "transfer_purse_to_accounts_hash";
const ACCESS_KEY_NAME: &str = "transfer_purse_to_accounts_access";

const ARG_AMOUNT: &str = "amount";
const ARG_SOURCE: &str = "source";
const ARG_TARGETS: &str = "targets";

const CONTRACT_VERSION: &str = "contract_version";

const PURSE_KEY_NAME: &str = "purse";

#[no_mangle]
pub extern "C" fn transfer() {
    let purse = runtime::get_key(PURSE_KEY_NAME)
        .unwrap_or_revert()
        .into_uref()
        .unwrap_or_revert();
    transfer_purse_to_accounts::delegate(purse);
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut tmp = EntryPoints::new();
        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME.to_string(),
            vec![
                Parameter::new(ARG_SOURCE, CLType::URef),
                Parameter::new(
                    ARG_TARGETS,
                    <BTreeMap<AccountHash, (U512, Option<u64>)>>::cl_type(),
                ),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        tmp.add_entry_point(entry_point);
        tmp
    };

    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let named_keys = {
        let purse = system::create_purse();
        system::transfer_from_purse_to_purse(account::get_main_purse(), purse, amount, None)
            .unwrap_or_revert();

        let mut named_keys = NamedKeys::new();
        named_keys.insert(PURSE_KEY_NAME.to_string(), purse.into());
        named_keys
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, Key::contract_entity_key(contract_hash));
}
