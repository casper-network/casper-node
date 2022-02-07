#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    runtime_args, CLType, CLTyped, ContractHash, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Parameter, RuntimeArgs,
};

const RECURSE_ENTRYPOINT: &str = "recurse";
const ARG_TARGET: &str = "target";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";

#[no_mangle]
pub extern "C" fn call() {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        RECURSE_ENTRYPOINT,
        vec![Parameter::new(ARG_TARGET, ContractHash::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));

    let (contract_hash, _contract_version) =
        storage::new_locked_contract(entry_points, None, None, None);

    runtime::put_key(CONTRACT_HASH_NAME, contract_hash.into());
}

#[no_mangle]
pub extern "C" fn recurse() {
    let target: ContractHash = runtime::get_named_arg(ARG_TARGET);
    let _ret: () = runtime::call_contract(
        target,
        RECURSE_ENTRYPOINT,
        runtime_args! { ARG_TARGET => target },
    );
}
