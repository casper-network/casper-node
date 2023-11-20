#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::{collections::BTreeSet, string::ToString, vec::Vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys},
    package::ENTITY_INITIAL_VERSION,
    runtime_args,
    system::{handle_payment, standard_payment},
    CLType, CLTyped, Key, PackageHash, Parameter, RuntimeArgs, URef, U512,
};

const PACKAGE_HASH_KEY: &str = "package_hash_key";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";
const RESTRICTED_CONTRACT: &str = "restricted_contract";
const RESTRICTED_SESSION: &str = "restricted_session";
const RESTRICTED_SESSION_CALLER: &str = "restricted_session_caller";
const UNRESTRICTED_CONTRACT_CALLER: &str = "unrestricted_contract_caller";
const RESTRICTED_CONTRACT_CALLER_AS_SESSION: &str = "restricted_contract_caller_as_session";
const UNCALLABLE_SESSION: &str = "uncallable_session";
const UNCALLABLE_CONTRACT: &str = "uncallable_contract";
const CALL_RESTRICTED_ENTRY_POINTS: &str = "call_restricted_entry_points";
const RESTRICTED_STANDARD_PAYMENT: &str = "restricted_standard_payment";
const ARG_PACKAGE_HASH: &str = "package_hash";

#[no_mangle]
pub extern "C" fn restricted_session() {}

#[no_mangle]
pub extern "C" fn restricted_contract() {}

#[no_mangle]
pub extern "C" fn restricted_session_caller() {
    let package_hash: Key = runtime::get_named_arg(ARG_PACKAGE_HASH);
    let contract_version = Some(ENTITY_INITIAL_VERSION);
    let contract_package_hash = package_hash
        .into_entity_hash_addr()
        .unwrap_or_revert()
        .into();
    runtime::call_versioned_contract(
        contract_package_hash,
        contract_version,
        RESTRICTED_SESSION,
        runtime_args! {},
    )
}

fn contract_caller() {
    let package_hash: Key = runtime::get_named_arg(ARG_PACKAGE_HASH);
    let contract_version = Some(ENTITY_INITIAL_VERSION);
    let contract_package_hash = package_hash.into_package_hash().unwrap_or_revert();
    let runtime_args = runtime_args! {};
    runtime::call_versioned_contract(
        contract_package_hash,
        contract_version,
        RESTRICTED_CONTRACT,
        runtime_args,
    )
}

#[no_mangle]
pub extern "C" fn unrestricted_contract_caller() {
    contract_caller();
}

#[no_mangle]
pub extern "C" fn restricted_contract_caller_as_session() {
    contract_caller();
}

#[no_mangle]
pub extern "C" fn uncallable_session() {}

#[no_mangle]
pub extern "C" fn uncallable_contract() {}

fn get_payment_purse() -> URef {
    runtime::call_contract(
        system::get_handle_payment(),
        handle_payment::METHOD_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    )
}

#[no_mangle]
pub extern "C" fn restricted_standard_payment() {
    let amount: U512 = runtime::get_named_arg(standard_payment::ARG_AMOUNT);

    let payment_purse = get_payment_purse();
    system::transfer_from_purse_to_purse(account::get_main_purse(), payment_purse, amount, None)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call_restricted_entry_points() {
    // We're aggressively removing exports that aren't exposed through contract header so test
    // ensures that those exports are still inside WASM.
    uncallable_session();
    uncallable_contract();
}

fn create_group(package_hash: PackageHash) -> URef {
    let new_uref_1 = storage::new_uref(());
    runtime::put_key("saved_uref", new_uref_1.into());

    let mut existing_urefs = BTreeSet::new();
    existing_urefs.insert(new_uref_1);

    let new_urefs = storage::create_contract_user_group(package_hash, "Group 1", 1, existing_urefs)
        .unwrap_or_revert();
    assert_eq!(new_urefs.len(), 1);
    new_urefs[0]
}

/// Restricted uref comes from creating a group and will be assigned to a smart contract
fn create_entry_points_1() -> EntryPoints {
    let mut entry_points = EntryPoints::new();
    let restricted_session = EntryPoint::new(
        RESTRICTED_SESSION.to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::groups(&["Group 1"]),
        EntryPointType::Session,
    );
    entry_points.add_entry_point(restricted_session);

    let restricted_contract = EntryPoint::new(
        RESTRICTED_CONTRACT.to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::groups(&["Group 1"]),
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(restricted_contract);

    let restricted_session_caller = EntryPoint::new(
        RESTRICTED_SESSION_CALLER.to_string(),
        vec![Parameter::new(ARG_PACKAGE_HASH, CLType::Key)],
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(restricted_session_caller);

    let restricted_contract = EntryPoint::new(
        RESTRICTED_CONTRACT.to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::groups(&["Group 1"]),
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(restricted_contract);

    let unrestricted_contract_caller = EntryPoint::new(
        UNRESTRICTED_CONTRACT_CALLER.to_string(),
        Vec::new(),
        CLType::I32,
        // Made public because we've tested deploy level auth into a contract in
        // RESTRICTED_CONTRACT entrypoint
        EntryPointAccess::Public,
        // NOTE: Public contract authorizes any contract call, because this contract has groups
        // uref in its named keys
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(unrestricted_contract_caller);

    let unrestricted_contract_caller_as_session = EntryPoint::new(
        RESTRICTED_CONTRACT_CALLER_AS_SESSION.to_string(),
        Vec::new(),
        CLType::I32,
        // Made public because we've tested deploy level auth into a contract in
        // RESTRICTED_CONTRACT entrypoint
        EntryPointAccess::Public,
        // NOTE: Public contract authorizes any contract call, because this contract has groups
        // uref in its named keys
        EntryPointType::Session,
    );
    entry_points.add_entry_point(unrestricted_contract_caller_as_session);

    let uncallable_session = EntryPoint::new(
        UNCALLABLE_SESSION.to_string(),
        Vec::new(),
        CLType::I32,
        // Made public because we've tested deploy level auth into a contract in
        // RESTRICTED_CONTRACT entrypoint
        EntryPointAccess::groups(&[]),
        // NOTE: Public contract authorizes any contract call, because this contract has groups
        // uref in its named keys
        EntryPointType::Session,
    );
    entry_points.add_entry_point(uncallable_session);

    let uncallable_contract = EntryPoint::new(
        UNCALLABLE_CONTRACT.to_string(),
        Vec::new(),
        CLType::I32,
        // Made public because we've tested deploy level auth into a contract in
        // RESTRICTED_CONTRACT entrypoint
        EntryPointAccess::groups(&[]),
        // NOTE: Public contract authorizes any contract call, because this contract has groups
        // uref in its named keys
        EntryPointType::Session,
    );
    entry_points.add_entry_point(uncallable_contract);

    // Directly calls entry_points that are protected with empty group of lists to verify that even
    // though they're not callable externally, they're still visible in the WASM.
    let call_restricted_entry_points = EntryPoint::new(
        CALL_RESTRICTED_ENTRY_POINTS.to_string(),
        Vec::new(),
        CLType::I32,
        // Made public because we've tested deploy level auth into a contract in
        // RESTRICTED_CONTRACT entrypoint
        EntryPointAccess::Public,
        // NOTE: Public contract authorizes any contract call, because this contract has groups
        // uref in its named keys
        EntryPointType::Session,
    );
    entry_points.add_entry_point(call_restricted_entry_points);

    let restricted_standard_payment = EntryPoint::new(
        RESTRICTED_STANDARD_PAYMENT.to_string(),
        vec![Parameter::new(
            standard_payment::ARG_AMOUNT,
            U512::cl_type(),
        )],
        CLType::Unit,
        EntryPointAccess::groups(&["Group 1"]),
        EntryPointType::Session,
    );
    entry_points.add_entry_point(restricted_standard_payment);

    entry_points
}

fn install_version_1(contract_package_hash: PackageHash, restricted_uref: URef) {
    let contract_named_keys = {
        let contract_variable = storage::new_uref(0);

        let mut named_keys = NamedKeys::new();
        named_keys.insert("contract_named_key".to_string(), contract_variable.into());
        named_keys.insert("restricted_uref".to_string(), restricted_uref.into());
        named_keys
    };

    let entry_points = create_entry_points_1();
    storage::add_contract_version(contract_package_hash, entry_points, contract_named_keys);
}

#[no_mangle]
pub extern "C" fn call() {
    // Session contract
    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();

    runtime::put_key(PACKAGE_HASH_KEY, contract_package_hash.into());
    runtime::put_key(PACKAGE_ACCESS_KEY, access_uref.into());

    let restricted_uref = create_group(contract_package_hash);

    install_version_1(contract_package_hash, restricted_uref);
}
