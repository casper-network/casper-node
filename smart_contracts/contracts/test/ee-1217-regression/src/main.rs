#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

// casper_contract is required for it's [global_alloc] as well as handlers (such as panic_handler)
use casper_contract::contract_api::{runtime, storage, system};
use casper_types::{
    runtime_args, system::auction, CLType, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, PublicKey, RuntimeArgs, U512,
};

const PACKAGE_NAME: &str = "call_auction";
const PACKAGE_ACCESS_KEY_NAME: &str = "call_auction_access";

const METHOD_ADD_BID_CONTRACT_NAME: &str = "add_bid_contract";
const METHOD_ADD_BID_SESSION_NAME: &str = "add_bid_session";
const METHOD_WITHDRAW_BID_CONTRACT_NAME: &str = "withdraw_bid_contract";
const METHOD_WITHDRAW_BID_SESSION_NAME: &str = "withdraw_bid_session";
const METHOD_DELEGATE_CONTRACT_NAME: &str = "delegate_contract";
const METHOD_DELEGATE_SESSION_NAME: &str = "delegate_session";
const METHOD_UNDELEGATE_CONTRACT_NAME: &str = "undelegate_contract";
const METHOD_UNDELEGATE_SESSION_NAME: &str = "undelegate_session";
const METHOD_ACTIVATE_BID_CONTRACT_NAME: &str = "activate_bid_contract";
const METHOD_ACTIVATE_BID_SESSION_NAME: &str = "activate_bid_session";

fn add_bid() {
    let public_key: PublicKey = runtime::get_named_arg(auction::ARG_PUBLIC_KEY);
    let auction = system::get_auction();
    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_AMOUNT => U512::one(),
        auction::ARG_DELEGATION_RATE => 42u8,
    };
    runtime::call_contract::<U512>(auction, auction::METHOD_ADD_BID, args);
}

#[no_mangle]
pub extern "C" fn add_bid_contract() {
    add_bid()
}

#[no_mangle]
pub extern "C" fn add_bid_session() {
    add_bid()
}

pub fn withdraw_bid() {
    let public_key: PublicKey = runtime::get_named_arg(auction::ARG_PUBLIC_KEY);
    let auction = system::get_auction();
    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_AMOUNT => U512::one(),
    };
    runtime::call_contract::<U512>(auction, auction::METHOD_WITHDRAW_BID, args);
}

#[no_mangle]
pub extern "C" fn withdraw_bid_contract() {
    withdraw_bid()
}

#[no_mangle]
pub extern "C" fn withdraw_bid_session() {
    withdraw_bid()
}

fn activate_bid() {
    let public_key: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR_PUBLIC_KEY);
    let auction = system::get_auction();
    let args = runtime_args! {
        auction::ARG_VALIDATOR_PUBLIC_KEY => public_key,
    };
    runtime::call_contract::<()>(auction, auction::METHOD_ACTIVATE_BID, args);
}

#[no_mangle]
pub extern "C" fn activate_bid_contract() {
    activate_bid()
}

#[no_mangle]
pub extern "C" fn activate_bid_session() {
    activate_bid()
}

#[no_mangle]
pub extern "C" fn delegate() {
    let delegator_public_key: PublicKey = runtime::get_named_arg(auction::ARG_DELEGATOR);
    let validator_public_key: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR);
    let auction = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator_public_key,
        auction::ARG_VALIDATOR => validator_public_key,
        auction::ARG_AMOUNT => U512::one(),
    };
    runtime::call_contract::<U512>(auction, auction::METHOD_DELEGATE, args);
}

#[no_mangle]
pub extern "C" fn delegate_contract() {
    delegate()
}

#[no_mangle]
pub extern "C" fn delegate_session() {
    delegate()
}

#[no_mangle]
pub extern "C" fn undelegate() {
    let delegator_public_key: PublicKey = runtime::get_named_arg(auction::ARG_DELEGATOR);
    let validator_public_key: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR);
    let auction = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator_public_key,
        auction::ARG_VALIDATOR => validator_public_key,
        auction::ARG_AMOUNT => U512::one(),
        auction::ARG_NEW_VALIDATOR => Option::<PublicKey>::None
    };
    runtime::call_contract::<U512>(auction, auction::METHOD_UNDELEGATE, args);
}

#[no_mangle]
pub extern "C" fn undelegate_contract() {
    undelegate()
}

#[no_mangle]
pub extern "C" fn undelegate_session() {
    undelegate()
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let add_bid_session_entry_point = EntryPoint::new(
            METHOD_ADD_BID_SESSION_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        let add_bid_contract_entry_point = EntryPoint::new(
            METHOD_ADD_BID_CONTRACT_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        let withdraw_bid_session_entry_point = EntryPoint::new(
            METHOD_WITHDRAW_BID_SESSION_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        let withdraw_bid_contract_entry_point = EntryPoint::new(
            METHOD_WITHDRAW_BID_CONTRACT_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        let delegate_session_entry_point = EntryPoint::new(
            METHOD_DELEGATE_SESSION_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        let delegate_contract_entry_point = EntryPoint::new(
            METHOD_DELEGATE_CONTRACT_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        let undelegate_session_entry_point = EntryPoint::new(
            METHOD_UNDELEGATE_SESSION_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        let undelegate_contract_entry_point = EntryPoint::new(
            METHOD_UNDELEGATE_CONTRACT_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        let activate_bid_session_entry_point = EntryPoint::new(
            METHOD_ACTIVATE_BID_SESSION_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        let activate_bid_contract_entry_point = EntryPoint::new(
            METHOD_ACTIVATE_BID_CONTRACT_NAME.to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(add_bid_session_entry_point);
        entry_points.add_entry_point(add_bid_contract_entry_point);
        entry_points.add_entry_point(withdraw_bid_session_entry_point);
        entry_points.add_entry_point(withdraw_bid_contract_entry_point);
        entry_points.add_entry_point(delegate_session_entry_point);
        entry_points.add_entry_point(delegate_contract_entry_point);
        entry_points.add_entry_point(undelegate_session_entry_point);
        entry_points.add_entry_point(undelegate_contract_entry_point);
        entry_points.add_entry_point(activate_bid_session_entry_point);
        entry_points.add_entry_point(activate_bid_contract_entry_point);
        entry_points
    };

    let (_contract_hash, _contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_NAME.to_string()),
        Some(PACKAGE_ACCESS_KEY_NAME.to_string()),
    );
}
