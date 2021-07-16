#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::ToString;

use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{handle_payment, mint},
    AccessRights, CLType, CLTyped, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints,
    Parameter, RuntimeArgs, URef, U512,
};

const HARDCODED_UREF: URef = URef::new([42; 32], AccessRights::READ_ADD_WRITE);

const PACKAGE_HASH_NAME: &str = "package_hash_name";
const ACCESS_UREF_NAME: &str = "uref_name";
const CONTRACT_HASH_NAME: &str = "contract_hash";

const ARG_SOURCE: &str = "source";
const ARG_RECIPIENT: &str = "recipient";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";
const ARG_REFUND_PURSE: &str = "refund_purse";

const METHOD_SEND_TO_ACCOUNT: &str = "send_to_account";
const METHOD_SEND_TO_PURSE: &str = "send_to_purse";
const METHOD_HARDCODED_PURSE_SRC: &str = "hardcoded_purse_src";
const METHOD_STORED_PAYMENT: &str = "stored_payment";
const METHOD_HARDCODED_PAYMENT: &str = "hardcoded_payment";

pub fn get_payment_purse() -> URef {
    runtime::call_contract(
        system::get_handle_payment(),
        handle_payment::METHOD_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    )
}

pub fn set_refund_purse(refund_purse: URef) {
    let args = runtime_args! {
        mint::ARG_PURSE => refund_purse,
    };
    runtime::call_contract(
        system::get_handle_payment(),
        handle_payment::METHOD_SET_REFUND_PURSE,
        args,
    )
}

#[no_mangle]
pub extern "C" fn send_to_account() {
    let source: URef = runtime::get_named_arg(ARG_SOURCE);
    let recipient: AccountHash = runtime::get_named_arg(ARG_RECIPIENT);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    system::transfer_from_purse_to_account(source, recipient, amount, None).unwrap();
}

#[no_mangle]
pub extern "C" fn send_to_purse() {
    let source: URef = runtime::get_named_arg(ARG_SOURCE);
    let target: URef = runtime::get_named_arg(ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    system::transfer_from_purse_to_purse(source, target, amount, None).unwrap();
}

#[no_mangle]
pub extern "C" fn hardcoded_purse_src() {
    let source = HARDCODED_UREF;
    let target = runtime::get_named_arg(ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    system::transfer_from_purse_to_purse(source, target, amount, None).unwrap();
}

#[no_mangle]
pub extern "C" fn stored_payment() {
    // Refund purse
    let refund_purse: URef = runtime::get_named_arg(ARG_REFUND_PURSE);
    // Who will be charged
    let source: URef = runtime::get_named_arg(ARG_SOURCE);
    // How much to pay for execution
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    // set refund purse to specified purse
    set_refund_purse(refund_purse);

    // get payment purse for current execution
    let payment_purse: URef = get_payment_purse();

    // transfer amount from named purse to payment purse, which will be used to pay for execution
    system::transfer_from_purse_to_purse(source, payment_purse, amount, None).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn hardcoded_payment() {
    // Refund purse
    let refund_purse: URef = runtime::get_named_arg(ARG_REFUND_PURSE);
    // Who will be charged
    let source: URef = HARDCODED_UREF;
    // How much to pay for execution
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    // set refund purse to specified purse
    set_refund_purse(refund_purse);

    // get payment purse for current execution
    let payment_purse: URef = get_payment_purse();

    // transfer amount from named purse to payment purse, which will be used to pay for execution
    system::transfer_from_purse_to_purse(source, payment_purse, amount, None).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call() {
    let mut entry_points = EntryPoints::new();

    let send_to_account = EntryPoint::new(
        METHOD_SEND_TO_ACCOUNT,
        vec![
            Parameter::new(ARG_SOURCE, URef::cl_type()),
            Parameter::new(ARG_RECIPIENT, AccountHash::cl_type()),
            Parameter::new(ARG_AMOUNT, CLType::U512),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    let send_to_purse = EntryPoint::new(
        METHOD_SEND_TO_PURSE,
        vec![
            Parameter::new(ARG_SOURCE, URef::cl_type()),
            Parameter::new(ARG_TARGET, URef::cl_type()),
            Parameter::new(ARG_AMOUNT, CLType::U512),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    let hardcoded_src = EntryPoint::new(
        METHOD_HARDCODED_PURSE_SRC,
        vec![
            Parameter::new(ARG_TARGET, URef::cl_type()),
            Parameter::new(ARG_AMOUNT, CLType::U512),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    let stored_payment = EntryPoint::new(
        METHOD_STORED_PAYMENT,
        vec![
            Parameter::new(ARG_REFUND_PURSE, URef::cl_type()),
            Parameter::new(ARG_SOURCE, URef::cl_type()),
            Parameter::new(ARG_AMOUNT, CLType::U512),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    let hardcoded_payment = EntryPoint::new(
        METHOD_HARDCODED_PAYMENT,
        vec![
            Parameter::new(ARG_REFUND_PURSE, URef::cl_type()),
            Parameter::new(ARG_AMOUNT, CLType::U512),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    entry_points.add_entry_point(send_to_account);
    entry_points.add_entry_point(send_to_purse);
    entry_points.add_entry_point(hardcoded_src);
    entry_points.add_entry_point(stored_payment);
    entry_points.add_entry_point(hardcoded_payment);

    let (contract_hash, _version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_HASH_NAME.to_string()),
        Some(ACCESS_UREF_NAME.to_string()),
    );
    runtime::put_key(CONTRACT_HASH_NAME, contract_hash.into());
}
