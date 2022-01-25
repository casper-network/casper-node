#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, format, string::ToString, vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system::transfer_from_purse_to_purse},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::{ContractHash, NamedKeys},
    ApiError, CLType, ContractVersion, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints,
    Parameter, RuntimeArgs, URef,
};

#[repr(u16)]
enum InstallerSessionError {
    FailedToTransfer = 101,
}

#[no_mangle]
pub extern "C" fn call_faucet() {
    faucet::delegate();
}

fn store() -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let faucet = EntryPoint::new(
            faucet::ENTRY_POINT_FAUCET,
            vec![
                Parameter::new(faucet::ARG_ID, CLType::Option(Box::new(CLType::U64))),
                Parameter::new(faucet::ARG_TARGET, CLType::PublicKey),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let init_purse = EntryPoint::new(
            faucet::ENTRY_POINT_INIT,
            vec![],
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let set_variables = EntryPoint::new(
            faucet::ENTRY_POINT_SET_VARIABLES,
            vec![
                Parameter::new(faucet::ARG_AVAILABLE_AMOUNT, CLType::U512),
                Parameter::new(faucet::ARG_TIME_INTERVAL, CLType::U64),
                Parameter::new(faucet::ARG_DISTRIBUTIONS_PER_INTERVAL, CLType::U64),
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
        named_keys.insert(faucet::INSTALLER.to_string(), runtime::get_caller().into());
        named_keys
    };

    storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(faucet::HASH_KEY_NAME.to_string()),
        Some(faucet::ACCESS_KEY_NAME.to_string()),
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let id: u64 = runtime::get_named_arg(faucet::ARG_ID);

    let (contract_hash, contract_version) = store();
    runtime::put_key(
        faucet::CONTRACT_VERSION,
        storage::new_uref(contract_version).into(),
    );
    runtime::put_key(faucet::CONTRACT_NAME, contract_hash.into());

    // init the newly installed contract,
    // keep track of the purse it returns so that it can be easily topped off later
    // then fund that purse so that the faucet has some token to distribute
    let purse: URef = runtime::call_contract(
        contract_hash,
        faucet::ENTRY_POINT_INIT,
        RuntimeArgs::default(),
    );
    runtime::put_key(&format!("faucet_{}", id), purse.into());
    let main_purse = account::get_main_purse();
    let amount = runtime::get_named_arg(faucet::ARG_AMOUNT);
    transfer_from_purse_to_purse(main_purse, purse, amount, Some(id)).unwrap_or_revert_with(
        ApiError::User(InstallerSessionError::FailedToTransfer as u16),
    );
}
