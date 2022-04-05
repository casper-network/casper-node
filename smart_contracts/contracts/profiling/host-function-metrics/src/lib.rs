#![no_std]

extern crate alloc;

use alloc::{boxed::Box, string::String, vec, vec::Vec};
use core::iter;

use rand::{distributions::Alphanumeric, rngs::SmallRng, Rng, SeedableRng};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::{AccountHash, ActionType, Weight},
    bytesrepr::Bytes,
    contracts::NamedKeys,
    runtime_args, ApiError, BlockTime, CLType, CLValue, ContractHash, ContractVersion, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints, Key, Parameter, Phase, RuntimeArgs, U512,
};

const MIN_FUNCTION_NAME_LENGTH: usize = 1;
const MAX_FUNCTION_NAME_LENGTH: usize = 100;

const NAMED_KEY_COUNT: usize = 100;
const MIN_NAMED_KEY_NAME_LENGTH: usize = 10;
// TODO - consider increasing to e.g. 1_000 once https://casperlabs.atlassian.net/browse/EE-966 is
//        resolved.
const MAX_NAMED_KEY_NAME_LENGTH: usize = 100;
const VALUE_FOR_ADDITION_1: u64 = 1;
const VALUE_FOR_ADDITION_2: u64 = 2;
const TRANSFER_AMOUNT: u64 = 1_000_000;

const ARG_SEED: &str = "seed";
const ARG_OTHERS: &str = "others";
const ARG_BYTES: &str = "bytes";

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
    Revert = 9,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> ApiError {
        ApiError::User(error as u16)
    }
}

fn create_random_names(rng: &mut SmallRng) -> impl Iterator<Item = String> + '_ {
    iter::repeat_with(move || {
        let key_length: usize = rng.gen_range(MIN_NAMED_KEY_NAME_LENGTH..MAX_NAMED_KEY_NAME_LENGTH);
        rng.sample_iter(&Alphanumeric)
            .map(char::from)
            .take(key_length)
            .collect::<String>()
    })
    .take(NAMED_KEY_COUNT)
}

fn truncate_named_keys(named_keys: NamedKeys, rng: &mut SmallRng) -> NamedKeys {
    let truncated_len = rng.gen_range(1..=named_keys.len());
    let mut vec = named_keys.into_iter().collect::<Vec<_>>();
    vec.truncate(truncated_len);
    vec.into_iter().collect()
}

// Executes the named key functions from the `runtime` module and most of the functions from the
// `storage` module.
fn large_function() {
    let seed: u64 = runtime::get_named_arg(ARG_SEED);
    let random_bytes: Bytes = runtime::get_named_arg(ARG_BYTES);

    let uref = storage::new_uref(random_bytes.clone());

    let mut rng = SmallRng::seed_from_u64(seed);
    let mut key_name = String::new();
    for random_name in create_random_names(&mut rng) {
        key_name = random_name;
        runtime::put_key(&key_name, Key::from(uref));
    }

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

    let keys_to_return = truncate_named_keys(named_keys, &mut rng);
    runtime::ret(CLValue::from_t(keys_to_return).unwrap_or_revert());
}

fn small_function() {
    if runtime::get_phase() != Phase::Session {
        runtime::revert(Error::GetPhase);
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let seed: u64 = runtime::get_named_arg(ARG_SEED);
    let (random_bytes, source_account, destination_account): (Bytes, AccountHash, AccountHash) =
        runtime::get_named_arg(ARG_OTHERS);
    let random_bytes: Vec<u8> = random_bytes.into();

    // ========== storage, execution and upgrading of contracts ====================================

    // Store large function with no named keys, then execute it to get named keys returned.
    let mut rng = SmallRng::seed_from_u64(seed);
    let large_function_name: String =
        "l".repeat(rng.gen_range(MIN_FUNCTION_NAME_LENGTH..=MAX_FUNCTION_NAME_LENGTH));

    let entry_point_name = &large_function_name;
    let runtime_args = runtime_args! {
        ARG_SEED => seed,
        ARG_BYTES => random_bytes.clone()
    };

    let (contract_hash, _contract_version) = store_function(entry_point_name, None);
    let named_keys: NamedKeys =
        runtime::call_contract(contract_hash, entry_point_name, runtime_args.clone());

    let (contract_hash, _contract_version) =
        store_function(entry_point_name, Some(named_keys.clone()));
    // Store large function with 10 named keys, then execute it.
    runtime::call_contract::<NamedKeys>(contract_hash, entry_point_name, runtime_args.clone());

    // Small function
    let small_function_name =
        "s".repeat(rng.gen_range(MIN_FUNCTION_NAME_LENGTH..=MAX_FUNCTION_NAME_LENGTH));

    let entry_point_name = &small_function_name;

    // Store small function with no named keys, then execute it.
    let (contract_hash, _contract_version) =
        store_function(entry_point_name, Some(NamedKeys::new()));
    runtime::call_contract::<()>(contract_hash, entry_point_name, runtime_args.clone());

    let (contract_hash, _contract_version) = store_function(entry_point_name, Some(named_keys));
    // Store small function with 10 named keys, then execute it.
    runtime::call_contract::<()>(contract_hash, entry_point_name, runtime_args);

    // ========== functions from `account` module ==================================================

    let main_purse = account::get_main_purse();
    account::set_action_threshold(ActionType::Deployment, Weight::new(1)).unwrap_or_revert();
    account::add_associated_key(destination_account, Weight::new(1)).unwrap_or_revert();
    account::update_associated_key(destination_account, Weight::new(1)).unwrap_or_revert();
    account::remove_associated_key(destination_account).unwrap_or_revert();

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

    system::transfer_from_purse_to_account(new_purse, destination_account, transfer_amount, None)
        .unwrap_or_revert();

    system::transfer_to_account(destination_account, transfer_amount, None).unwrap_or_revert();

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

    runtime::print(&String::from_utf8_lossy(&random_bytes));

    runtime::revert(Error::Revert);
}

fn store_function(
    entry_point_name: &str,
    named_keys: Option<NamedKeys>,
) -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            entry_point_name,
            vec![
                Parameter::new(ARG_SEED, CLType::U64),
                Parameter::new(ARG_BYTES, CLType::List(Box::new(CLType::U8))),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    storage::new_contract(entry_points, named_keys, None, None)
}

#[rustfmt::skip] #[no_mangle] pub extern "C" fn s() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn sssssssssssssssssssssssssss() { small_function()
}
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C"
fn ssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern
"C" fn sssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub
extern "C" fn ssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub
extern "C" fn ssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub
extern "C" fn ssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle]
pub extern "C" fn ssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn sssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function()
}
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() { small_function()
}
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss() {
small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss()
{ small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss()
{ small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss()
{ small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss()
{ small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss()
{ small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss()
{ small_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
ssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss()
{ small_function() }

#[rustfmt::skip] #[no_mangle] pub extern "C" fn l() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn ll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn lllllllllllllllllllllllllll() { large_function()
}
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C"
fn llllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern
"C" fn lllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub
extern "C" fn llllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub
extern "C" fn llllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub
extern "C" fn llllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn llllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle]
pub extern "C" fn llllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip]
#[no_mangle] pub extern "C" fn lllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::
skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function()
}
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() { large_function()
}
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll() {
large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll()
{ large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll()
{ large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll()
{ large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll()
{ large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll()
{ large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
lllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll()
{ large_function() }
#[rustfmt::skip] #[no_mangle] pub extern "C" fn
llllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllllll()
{ large_function() }
