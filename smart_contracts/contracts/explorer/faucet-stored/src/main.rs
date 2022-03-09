#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, format, string::ToString, vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::NamedKeys, ApiError, CLType, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Parameter, PublicKey, URef, U512,
};

#[repr(u16)]
enum InstallerSessionError {
    FailedToTransfer = 101,
}

#[no_mangle]
pub extern "C" fn call_faucet() {
    faucet::delegate();
}

fn build_named_keys_and_purse() -> (NamedKeys, URef) {
    let mut named_keys = NamedKeys::new();
    let purse = system::create_purse();
    named_keys.insert(faucet::FAUCET_PURSE.to_string(), purse.into());

    named_keys.insert(faucet::INSTALLER.to_string(), runtime::get_caller().into());
    named_keys.insert(
        faucet::TIME_INTERVAL.to_string(),
        storage::new_uref(faucet::TWO_HOURS_AS_MILLIS).into(),
    );
    named_keys.insert(
        faucet::LAST_DISTRIBUTION_TIME.to_string(),
        storage::new_uref(0u64).into(),
    );
    named_keys.insert(
        faucet::AVAILABLE_AMOUNT.to_string(),
        storage::new_uref(U512::zero()).into(),
    );
    named_keys.insert(
        faucet::REMAINING_AMOUNT.to_string(),
        storage::new_uref(U512::zero()).into(),
    );
    named_keys.insert(
        faucet::DISTRIBUTIONS_PER_INTERVAL.to_string(),
        storage::new_uref(0u64).into(),
    );
    named_keys.insert(
        faucet::AUTHORIZED_ACCOUNT.to_string(),
        storage::new_uref(None::<PublicKey>).into(),
    );

    (named_keys, purse)
}

#[no_mangle]
pub extern "C" fn call() {
    let id: u64 = runtime::get_named_arg(faucet::ARG_ID);
    let (faucet_named_keys, faucet_purse) = build_named_keys_and_purse();

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

        let set_variables = EntryPoint::new(
            faucet::ENTRY_POINT_SET_VARIABLES,
            vec![
                Parameter::new(
                    faucet::ARG_AVAILABLE_AMOUNT,
                    CLType::Option(Box::new(CLType::U512)),
                ),
                Parameter::new(
                    faucet::ARG_TIME_INTERVAL,
                    CLType::Option(Box::new(CLType::U64)),
                ),
                Parameter::new(
                    faucet::ARG_DISTRIBUTIONS_PER_INTERVAL,
                    CLType::Option(Box::new(CLType::U64)),
                ),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let authorize_to = EntryPoint::new(
            faucet::ENTRY_POINT_AUTHORIZE_TO,
            vec![Parameter::new(
                faucet::ARG_TARGET,
                CLType::Option(Box::new(CLType::PublicKey)),
            )],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(faucet);
        entry_points.add_entry_point(set_variables);
        entry_points.add_entry_point(authorize_to);

        entry_points
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        Some(faucet_named_keys),
        Some(faucet::HASH_KEY_NAME.to_string()),
        Some(faucet::ACCESS_KEY_NAME.to_string()),
    );

    runtime::put_key(
        faucet::CONTRACT_VERSION,
        storage::new_uref(contract_version).into(),
    );
    runtime::put_key(faucet::CONTRACT_NAME, contract_hash.into());
    runtime::put_key(&format!("faucet_{}", id), faucet_purse.into());

    let main_purse = account::get_main_purse();
    let amount = runtime::get_named_arg(faucet::ARG_AMOUNT);

    system::transfer_from_purse_to_purse(main_purse, faucet_purse, amount, Some(id))
        .unwrap_or_revert_with(ApiError::User(
            InstallerSessionError::FailedToTransfer as u16,
        ));
}
