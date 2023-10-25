#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use casper_contract::contract_api::{runtime, storage};

use casper_types::{
    addressable_entity::NamedKeys, CLType, CLTyped, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Group, Key, Parameter,
};
use gh_1470_regression::{
    Arg1Type, Arg2Type, Arg3Type, Arg4Type, Arg5Type, ARG1, ARG2, ARG3, ARG4, ARG5,
    CONTRACT_HASH_NAME, GROUP_LABEL, GROUP_UREF_NAME, PACKAGE_HASH_NAME,
    RESTRICTED_DO_NOTHING_ENTRYPOINT, RESTRICTED_WITH_EXTRA_ARG_ENTRYPOINT,
};

#[no_mangle]
pub extern "C" fn restricted_do_nothing_contract() {
    let _arg1: Arg1Type = runtime::get_named_arg(ARG1);
    let _arg2: Arg2Type = runtime::get_named_arg(ARG2);

    // ARG3 is defined in entrypoint but optional and might not be passed in all cases
}

#[no_mangle]
pub extern "C" fn restricted_with_extra_arg() {
    let _arg1: Arg1Type = runtime::get_named_arg(ARG1);
    let _arg2: Arg2Type = runtime::get_named_arg(ARG2);
    let _arg3: Arg3Type = runtime::get_named_arg(ARG3);

    // Those arguments are not present in entry point definition but are always passed by caller
    let _arg4: Arg4Type = runtime::get_named_arg(ARG4);
    let _arg5: Arg5Type = runtime::get_named_arg(ARG5);
}

#[no_mangle]
pub extern "C" fn call() {
    let (contract_package_hash, _access_uref) = storage::create_contract_package_at_hash();

    let admin_group = storage::create_contract_user_group(
        contract_package_hash,
        GROUP_LABEL,
        1,
        Default::default(),
    )
    .unwrap();

    runtime::put_key(GROUP_UREF_NAME, admin_group[0].into());

    let mut entry_points = EntryPoints::new();

    entry_points.add_entry_point(EntryPoint::new(
        RESTRICTED_DO_NOTHING_ENTRYPOINT,
        vec![
            Parameter::new(ARG2, Arg2Type::cl_type()),
            Parameter::new(ARG1, Arg1Type::cl_type()),
            Parameter::new(ARG3, Arg3Type::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Groups(vec![Group::new(GROUP_LABEL)]),
        EntryPointType::AddressableEntity,
    ));

    entry_points.add_entry_point(EntryPoint::new(
        RESTRICTED_WITH_EXTRA_ARG_ENTRYPOINT,
        vec![
            Parameter::new(ARG3, Arg3Type::cl_type()),
            Parameter::new(ARG2, Arg2Type::cl_type()),
            Parameter::new(ARG1, Arg1Type::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Groups(vec![Group::new(GROUP_LABEL)]),
        EntryPointType::AddressableEntity,
    ));

    let named_keys = NamedKeys::new();

    let (contract_hash, _) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    runtime::put_key(CONTRACT_HASH_NAME, Key::contract_entity_key(contract_hash));
    runtime::put_key(PACKAGE_HASH_NAME, contract_package_hash.into());
}
