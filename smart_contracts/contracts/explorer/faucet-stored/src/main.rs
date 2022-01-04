#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, format, string::ToString, vec};

use casper_contract::{
    contract_api,
    contract_api::{account, runtime, storage, system::transfer_from_purse_to_purse},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    contracts::{ContractHash, NamedKeys},
    ApiError, CLType, CLTyped, ContractVersion, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Parameter, RuntimeArgs, URef,
};

#[repr(u16)]
enum InstallerSessionError {
    FailedToTransfer = 101,
}

const ARG_ID: &str = "id";
const ARG_AMOUNT: &str = "amount";
const ARG_AVAILABLE_AMOUNT: &str = "available_amount";
const ARG_TIME_INTERVAL: &str = "time_increment";
const ENTRY_POINT_FAUCET: &str = "call_faucet";
const ENTRY_POINT_INIT: &str = "init";
const ENTRY_POINT_SET_VARIABLES: &str = "set_variables";
const INSTALLER: &str = "installer";
const CONTRACT_NAME: &str = "faucet";
const HASH_KEY_NAME: &str = "faucet_package";
const ACCESS_KEY_NAME: &str = "faucet_package_access";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn call_faucet() {
    faucet::delegate();
}

fn store() -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let faucet = EntryPoint::new(
            ENTRY_POINT_FAUCET,
            vec![Parameter::new(
                ARG_ID,
                CLType::Option(Box::new(CLType::U64)),
            )],
            CLType::U512,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let init_purse = EntryPoint::new(
            ENTRY_POINT_INIT,
            vec![],
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let set_variables = EntryPoint::new(
            ENTRY_POINT_SET_VARIABLES,
            vec![
                Parameter::new(ARG_AVAILABLE_AMOUNT, CLType::U512),
                Parameter::new(ARG_TIME_INTERVAL, CLType::U64),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(faucet);
        entry_points.add_entry_point(init_purse);
        entry_points.add_entry_point(set_variables);

        entry_points
    };

    let named_keys = {
        let mut named_keys = NamedKeys::new();
        named_keys.insert(INSTALLER.to_string(), runtime::get_caller().into());
        named_keys
    };

    storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let id: u64 = runtime::get_named_arg(ARG_ID);

    let (contract_hash, contract_version) = store();
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(CONTRACT_NAME, contract_hash.into());

    // init the newly installed contract,
    // keep track of the purse it returns so that it can be easily topped off later
    // then fund that purse so that the faucet has some token to distribute
    let purse: URef =
        runtime::call_contract(contract_hash, ENTRY_POINT_INIT, RuntimeArgs::default());
    runtime::put_key(&format!("faucet_{}", id), purse.into());
    let main_purse = account::get_main_purse();
    let amount = runtime::get_named_arg(ARG_AMOUNT);
    transfer_from_purse_to_purse(main_purse, purse, amount, Some(id)).unwrap_or_revert_with(
        ApiError::User(InstallerSessionError::FailedToTransfer as u16),
    );
}
