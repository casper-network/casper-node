#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use core::iter;

use alloc::{boxed::Box, string::String, vec::Vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::{AccountHash, ActionType, Weight},
    bytesrepr::Bytes,
    contracts::NamedKeys,
    runtime_args, ApiError, BlockTime, CLType, CLTyped, CLValue, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, Key, Parameter, Phase, RuntimeArgs, U512,
};

const DO_NOTHING_NAME: &str = "do_nothing";
const DO_SOMETHING_NAME: &str = "do_something";
const DO_HOST_FUNCTION_CALLS_NAME: &str = "do_host_function_calls";
const HASH_KEY_NAME: &str = "contract_package";
const CONTRACT_KEY_NAME: &str = "contract";
const CALLS_DO_NOTHING_LEVEL1_NAME: &str = "calls_do_nothing_level1";
const CALLS_DO_NOTHING_LEVEL2_NAME: &str = "calls_do_nothing_level2";
const TRANSFER_AMOUNT: u64 = 1_000_000;
const ARG_SOURCE_ACCOUNT: &str = "source_account";
const ARG_KEY_NAME: &str = "seed";
const ARG_BYTES: &str = "bytes";
const NAMED_KEY_COUNT: usize = 10;
const VALUE_FOR_ADDITION_1: u64 = 1;
const VALUE_FOR_ADDITION_2: u64 = 2;
const SHORT_FUNCTION_NAME_1: &str = "s";
const SHORT_FUNCTION_NAME_100: &str = "sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss";
const LONG_FUNCTION_NAME_1: &str = "l";
const LONG_FUNCTION_NAME_100: &str = "llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll";
const ARG_SIZE_FUNCTION_NAME: &str = "arg_size_function";
const ARG_SIZE_FUNCTION_CALL_1_NAME: &str = "arg_size_function_call_1";
const ARG_SIZE_FUNCTION_CALL_100_NAME: &str = "arg_size_function_call_100";

// A destination account hash that does not necessarily must exists
const DESTINATION_ACCOUNT_HASH: AccountHash = AccountHash::new([0x0A; 32]);

#[repr(u16)]
enum Error {
    GetCaller = 0,
    GetBlockTime = 1,
    GetPhase = 2,
    HasKey = 3,
    GetKey = 4,
    NamedKeys = 5,
    ReadOrRevert = 6,
    IsValidURef = 7,
    Transfer = 8,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> ApiError {
        ApiError::User(error as u16)
    }
}

#[no_mangle]
pub extern "C" fn do_nothing() {}

#[no_mangle]
pub extern "C" fn do_something() {
    let _result = uses_opcodes();
}

fn uses_opcodes() -> Box<u64> {
    let long_bytes = DO_NOTHING_NAME
        .chars()
        .chain(DO_SOMETHING_NAME.chars())
        .chain(DO_HOST_FUNCTION_CALLS_NAME.chars())
        .chain(HASH_KEY_NAME.chars())
        .chain(CONTRACT_KEY_NAME.chars());

    // Exercises various opcodes. Should cost more than "do_nothing".
    let mut amount = Box::new(1);
    for (i, c) in long_bytes.enumerate() {
        *amount += c as u64;
        *amount *= c as u64;
        *amount ^= i as u64;
        *amount |= i as u64;
        *amount &= i as u64;
    }

    amount
}

#[no_mangle]
pub extern "C" fn small_function() {
    if runtime::get_phase() != Phase::Session {
        runtime::revert(Error::GetPhase);
    }
}

#[no_mangle]
pub extern "C" fn s() {
    small_function();
}

#[no_mangle]
pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss(
) {
    small_function();
}

#[no_mangle]
pub extern "C" fn l() {
    account_function()
}

#[no_mangle]
pub extern "C" fn llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll(
) {
    account_function()
}

#[no_mangle]
pub extern "C" fn arg_size_function() {
    let _bytes: Bytes = runtime::get_named_arg(ARG_BYTES);
}

// Executes the named key functions from the `runtime` module and most of the functions from the
// `storage` module.
#[no_mangle]
pub extern "C" fn storage_function() {
    let key_name: String = runtime::get_named_arg(ARG_KEY_NAME);
    let random_bytes: Bytes = runtime::get_named_arg(ARG_BYTES);

    let uref = storage::new_uref(random_bytes.clone());

    runtime::put_key(&key_name, Key::from(uref));

    if !runtime::has_key(&key_name) {
        runtime::revert(Error::HasKey);
    }

    if runtime::get_key(&key_name) != Some(Key::from(uref)) {
        runtime::revert(Error::GetKey);
    }

    runtime::remove_key(&key_name);

    let named_keys = runtime::list_named_keys();
    if named_keys.len() != NAMED_KEY_COUNT - 1 {
        runtime::revert(Error::NamedKeys)
    }

    storage::write(uref, random_bytes.clone());
    let retrieved_value: Bytes = storage::read_or_revert(uref);
    if retrieved_value != random_bytes {
        runtime::revert(Error::ReadOrRevert);
    }

    storage::write(uref, VALUE_FOR_ADDITION_1);
    storage::add(uref, VALUE_FOR_ADDITION_2);

    let keys_to_return = runtime::list_named_keys();
    runtime::ret(CLValue::from_t(keys_to_return).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn account_function() {
    let source_account: AccountHash = runtime::get_named_arg(ARG_SOURCE_ACCOUNT);

    // ========== functions from `account` module ==================================================

    let main_purse = account::get_main_purse();
    account::set_action_threshold(ActionType::Deployment, Weight::new(1)).unwrap_or_revert();
    account::add_associated_key(DESTINATION_ACCOUNT_HASH, Weight::new(1)).unwrap_or_revert();
    account::update_associated_key(DESTINATION_ACCOUNT_HASH, Weight::new(1)).unwrap_or_revert();
    account::remove_associated_key(DESTINATION_ACCOUNT_HASH).unwrap_or_revert();

    // ========== functions from `system` module ===================================================

    let _ = system::get_mint();

    let new_purse = system::create_purse();

    let transfer_amount = U512::from(TRANSFER_AMOUNT);
    system::transfer_from_purse_to_purse(main_purse, new_purse, transfer_amount, None)
        .unwrap_or_revert();

    let balance = system::get_purse_balance(new_purse).unwrap_or_revert();
    if balance != transfer_amount {
        runtime::revert(Error::Transfer);
    }

    system::transfer_from_purse_to_account(
        new_purse,
        DESTINATION_ACCOUNT_HASH,
        transfer_amount,
        None,
    )
    .unwrap_or_revert();

    system::transfer_to_account(DESTINATION_ACCOUNT_HASH, transfer_amount, None).unwrap_or_revert();

    // ========== remaining functions from `runtime` module ========================================

    if !runtime::is_valid_uref(main_purse) {
        runtime::revert(Error::IsValidURef);
    }

    if runtime::get_blocktime() != BlockTime::new(0) {
        runtime::revert(Error::GetBlockTime);
    }

    if runtime::get_caller() != source_account {
        runtime::revert(Error::GetCaller);
    }
}

#[no_mangle]
pub extern "C" fn calls_do_nothing_level1() {
    let contract_package_hash = runtime::get_key(HASH_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have key")
        .into();
    runtime::call_versioned_contract(
        contract_package_hash,
        None,
        DO_NOTHING_NAME,
        RuntimeArgs::default(),
    )
}

#[no_mangle]
pub extern "C" fn calls_do_nothing_level2() {
    let contract_package_hash = runtime::get_key(HASH_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have key")
        .into();
    runtime::call_versioned_contract(
        contract_package_hash,
        None,
        CALLS_DO_NOTHING_LEVEL1_NAME,
        RuntimeArgs::default(),
    )
}

fn measure_arg_size(bytes: usize) {
    let contract_package_hash = runtime::get_key(HASH_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have key")
        .into();

    let argument: Vec<u8> = iter::repeat(b'1').take(bytes).collect();

    runtime::call_versioned_contract::<()>(
        contract_package_hash,
        None,
        ARG_SIZE_FUNCTION_NAME,
        runtime_args! {
            ARG_BYTES => argument,
        },
    );
}

#[no_mangle]
pub extern "C" fn arg_size_function_call_1() {
    measure_arg_size(0);
}

#[no_mangle]
pub extern "C" fn arg_size_function_call_100() {
    measure_arg_size(100);
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            DO_NOTHING_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            DO_SOMETHING_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            CALLS_DO_NOTHING_LEVEL1_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            CALLS_DO_NOTHING_LEVEL2_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            SHORT_FUNCTION_NAME_1,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            SHORT_FUNCTION_NAME_100,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            LONG_FUNCTION_NAME_1,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            LONG_FUNCTION_NAME_100,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            ARG_SIZE_FUNCTION_NAME,
            vec![Parameter::new(ARG_BYTES, <Vec<u8>>::cl_type())],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            "account_function",
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            "storage_function",
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            ARG_SIZE_FUNCTION_CALL_1_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            ARG_SIZE_FUNCTION_CALL_100_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        entry_points
    };

    let (contract_package_hash, _access_uref) = storage::create_contract_package_at_hash();

    runtime::put_key(&HASH_KEY_NAME, contract_package_hash.into());

    let mut named_keys = NamedKeys::new();
    named_keys.insert(HASH_KEY_NAME.into(), contract_package_hash.into());

    let (contract_hash, _version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);
    runtime::put_key(&CONTRACT_KEY_NAME, contract_hash.into());
}
