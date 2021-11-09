#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};

use casper_types::{
    account::AccountHash,
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys, Parameter},
    CLType, CLTyped, Key, U512,
};

const FAUCET_NAME: &str = "faucet";
const PACKAGE_HASH_KEY_NAME: &str = "gh_2280";
const HASH_KEY_NAME: &str = "gh_2280_hash";
const ACCESS_KEY_NAME: &str = "gh_2280_access";
const ARG_TARGET: &str = "target";
const CONTRACT_VERSION: &str = "contract_version";
const ARG_FAUCET_FUNDS: &str = "faucet_initial_balance";
const FAUCET_FUNDS_KEY: &str = "faucet_funds";

#[no_mangle]
pub extern "C" fn faucet() {
    let purse_uref = runtime::get_key(FAUCET_FUNDS_KEY)
        .and_then(Key::into_uref)
        .unwrap_or_revert();

    let account_hash: AccountHash = runtime::get_named_arg(ARG_TARGET);
    system::transfer_from_purse_to_account(purse_uref, account_hash, U512::from(1u64), None)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let faucet_entrypoint = EntryPoint::new(
            FAUCET_NAME.to_string(),
            vec![Parameter::new(ARG_TARGET, AccountHash::cl_type())],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(faucet_entrypoint);
        entry_points
    };

    let faucet_initial_balance: U512 = runtime::get_named_arg(ARG_FAUCET_FUNDS);

    let named_keys = {
        let faucet_funds = {
            let purse = system::create_purse();

            let id: Option<u64> = None;
            system::transfer_from_purse_to_purse(
                account::get_main_purse(),
                purse,
                faucet_initial_balance,
                id,
            )
            .unwrap_or_revert();

            purse
        };

        let mut named_keys = NamedKeys::new();

        named_keys.insert(FAUCET_FUNDS_KEY.to_string(), faucet_funds.into());

        named_keys
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, contract_hash.into());
}
