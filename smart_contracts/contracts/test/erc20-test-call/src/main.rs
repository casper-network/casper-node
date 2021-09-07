#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::String, vec};

use casper_contract::{
    self,
    contract_api::{runtime, storage},
};
use casper_types::{
    account::AccountHash, runtime_args, CLTyped, ContractHash, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, Key, Parameter, RuntimeArgs, U512,
};

const CHECK_BALANCE_OF_ENTRYPOINT: &str = "check_balance_of";
const CHECK_ALLOWANCE_OF_ENTRYPOINT: &str = "check_allowance_of";
const ARG_TOKEN_CONTRACT: &str = "token_contract";
const ARG_ADDRESS: &str = "address";
const ARG_OWNER: &str = "owner";
const ARG_SPENDER: &str = "spender";
const RESULT_KEY: &str = "result";
const ERC20_TEST_CALL_KEY: &str = "erc20_test_call";

#[no_mangle]
extern "C" fn check_balance_of() {
    let token_contract: ContractHash = runtime::get_named_arg(ARG_TOKEN_CONTRACT);
    let address: AccountHash = runtime::get_named_arg(ARG_ADDRESS);

    let balance_args = runtime_args! {
        casper_erc20::constants::ARG_ADDRESS => address,
    };
    let result: U512 = runtime::call_contract(
        token_contract,
        casper_erc20::constants::METHOD_BALANCE_OF,
        balance_args,
    );

    match runtime::get_key(RESULT_KEY) {
        Some(Key::URef(uref)) => storage::write(uref, result),
        Some(_) => unreachable!(),
        None => {
            let new_uref = storage::new_uref(result);
            runtime::put_key(RESULT_KEY, new_uref.into());
        }
    }
}

#[no_mangle]
extern "C" fn check_allowance_of() {
    let token_contract: ContractHash = runtime::get_named_arg(ARG_TOKEN_CONTRACT);
    let owner: AccountHash = runtime::get_named_arg(ARG_OWNER);
    let spender: AccountHash = runtime::get_named_arg(ARG_SPENDER);

    let allowance_args = runtime_args! {
        casper_erc20::constants::ARG_OWNER => owner,
        casper_erc20::constants::ARG_SPENDER => spender,
    };
    let result: U512 = runtime::call_contract(
        token_contract,
        casper_erc20::constants::METHOD_ALLOWANCE,
        allowance_args,
    );

    match runtime::get_key(RESULT_KEY) {
        Some(Key::URef(uref)) => storage::write(uref, result),
        Some(_) => unreachable!(),
        None => {
            let new_uref = storage::new_uref(result);
            runtime::put_key(RESULT_KEY, new_uref.into());
        }
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let mut entry_points = EntryPoints::new();
    let check_balance_of_entrypoint = EntryPoint::new(
        String::from(CHECK_BALANCE_OF_ENTRYPOINT),
        vec![
            Parameter::new(ARG_TOKEN_CONTRACT, ContractHash::cl_type()),
            Parameter::new(ARG_ADDRESS, AccountHash::cl_type()),
        ],
        <()>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    let check_allowance_of_entrypoint = EntryPoint::new(
        String::from(CHECK_ALLOWANCE_OF_ENTRYPOINT),
        vec![
            Parameter::new(ARG_TOKEN_CONTRACT, ContractHash::cl_type()),
            Parameter::new(ARG_OWNER, AccountHash::cl_type()),
            Parameter::new(ARG_SPENDER, AccountHash::cl_type()),
        ],
        <()>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    entry_points.add_entry_point(check_balance_of_entrypoint);
    entry_points.add_entry_point(check_allowance_of_entrypoint);

    let (contract_hash, _version) = storage::new_contract(entry_points, None, None, None);

    runtime::put_key(ERC20_TEST_CALL_KEY, contract_hash.into());
}
