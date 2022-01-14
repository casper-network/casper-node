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
    bytesrepr::FromBytes,
    contracts::NamedKeys,
    runtime_args,
    system::auction::{self, DelegationRate},
    CLType, CLTyped, CLValue, ContractPackageHash, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Key, Parameter, PublicKey, RuntimeArgs, U512,
};

const METHOD_ADD_BID_PROXY_CALL_1: &str = "add_bid_proxy_call_1";
const METHOD_ADD_BID_PROXY_CALL: &str = "add_bid_proxy_call";

const METHOD_WITHDRAW_PROXY_CALL: &str = "withdraw_proxy_call";
const METHOD_WITHDRAW_PROXY_CALL_1: &str = "withdraw_proxy_call_1";

const METHOD_DELEGATE_PROXY_CALL: &str = "delegate_proxy_call";
const METHOD_DELEGATE_PROXY_CALL_1: &str = "delegate_proxy_call_1";

const METHOD_UNDELEGATE_PROXY_CALL: &str = "undelegate_proxy_call";
const METHOD_UNDELEGATE_PROXY_CALL_1: &str = "undelegate_proxy_call_1";

const METHOD_ACTIVATE_BID_CALL: &str = "activate_bid_proxy_call";
const METHOD_ACTIVATE_BID_CALL_1: &str = "activate_bid_proxy_call_1";

const PACKAGE_HASH_NAME: &str = "package_hash_name";
const ACCESS_UREF_NAME: &str = "uref_name";
const CONTRACT_HASH_NAME: &str = "contract_hash";

fn forwarded_add_bid_args() -> RuntimeArgs {
    let public_key: PublicKey = runtime::get_named_arg(auction::ARG_PUBLIC_KEY);
    let delegation_rate: DelegationRate = runtime::get_named_arg(auction::ARG_DELEGATION_RATE);
    let amount: U512 = runtime::get_named_arg(auction::ARG_AMOUNT);

    runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_DELEGATION_RATE => delegation_rate,
        auction::ARG_AMOUNT => amount,
    }
}

fn forwarded_withdraw_bid_args() -> RuntimeArgs {
    let public_key: PublicKey = runtime::get_named_arg(auction::ARG_PUBLIC_KEY);
    let amount: U512 = runtime::get_named_arg(auction::ARG_AMOUNT);

    runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_AMOUNT => amount,
    }
}

fn forwarded_delegate_args() -> RuntimeArgs {
    let delegator: PublicKey = runtime::get_named_arg(auction::ARG_DELEGATOR);
    let validator: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR);
    let amount: U512 = runtime::get_named_arg(auction::ARG_AMOUNT);

    runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_AMOUNT => amount,
    }
}

fn forwarded_undelegate_args() -> RuntimeArgs {
    let delegator: PublicKey = runtime::get_named_arg(auction::ARG_DELEGATOR);
    let validator: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR);
    let amount: U512 = runtime::get_named_arg(auction::ARG_AMOUNT);

    runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_AMOUNT => amount,
        auction::ARG_NEW_VALIDATOR => Option::<PublicKey>::None
    }
}

fn forwarded_activate_bid_args() -> RuntimeArgs {
    let validator_public_key: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR_PUBLIC_KEY);

    runtime_args! {
        auction::ARG_VALIDATOR_PUBLIC_KEY => validator_public_key,
    }
}

#[no_mangle]
pub extern "C" fn withdraw_proxy_call_1() {
    let auction_contract_hash = system::get_auction();

    let withdraw_bid_args = forwarded_withdraw_bid_args();

    let result: U512 = runtime::call_contract(
        auction_contract_hash,
        auction::METHOD_WITHDRAW_BID,
        withdraw_bid_args,
    );

    runtime::ret(CLValue::from_t(result).unwrap_or_revert());
}

fn forward_call_to_this<T: CLTyped + FromBytes>(entry_point: &str, runtime_args: RuntimeArgs) -> T {
    let this = runtime::get_key(PACKAGE_HASH_NAME)
        .and_then(Key::into_hash)
        .map(ContractPackageHash::new)
        .unwrap_or_revert();
    runtime::call_versioned_contract(this, None, entry_point, runtime_args)
}

fn call_auction<T: CLTyped + FromBytes>(entry_point: &str, args: RuntimeArgs) -> T {
    runtime::call_contract(system::get_auction(), entry_point, args)
}

#[no_mangle]
pub extern "C" fn add_bid_proxy_call() {
    forward_call_to_this(METHOD_ADD_BID_PROXY_CALL_1, forwarded_add_bid_args())
}

#[no_mangle]
pub extern "C" fn add_bid_proxy_call_1() {
    let _result: U512 = call_auction(auction::METHOD_ADD_BID, forwarded_add_bid_args());
}

#[no_mangle]
pub extern "C" fn withdraw_proxy_call() {
    let _result: U512 =
        forward_call_to_this(METHOD_WITHDRAW_PROXY_CALL_1, forwarded_withdraw_bid_args());
}

#[no_mangle]
pub extern "C" fn delegate_proxy_call_1() {
    let result: U512 = call_auction(auction::METHOD_DELEGATE, forwarded_delegate_args());
    runtime::ret(CLValue::from_t(result).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn delegate_proxy_call() {
    let _result: U512 =
        forward_call_to_this(METHOD_DELEGATE_PROXY_CALL_1, forwarded_delegate_args());
}

#[no_mangle]
pub extern "C" fn undelegate_proxy_call_1() {
    let result: U512 = call_auction(auction::METHOD_UNDELEGATE, forwarded_undelegate_args());
    runtime::ret(CLValue::from_t(result).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn undelegate_proxy_call() {
    let _result: U512 =
        forward_call_to_this(METHOD_UNDELEGATE_PROXY_CALL_1, forwarded_undelegate_args());
}

#[no_mangle]
pub extern "C" fn activate_bid_proxy_call_1() {
    call_auction::<()>(auction::METHOD_ACTIVATE_BID, forwarded_activate_bid_args());
}

#[no_mangle]
pub extern "C" fn activate_bid_proxy_call() {
    forward_call_to_this(METHOD_ACTIVATE_BID_CALL_1, forwarded_activate_bid_args())
}

#[no_mangle]
pub extern "C" fn call() {
    let mut entry_points = EntryPoints::new();

    let add_bid_proxy_call_1 = EntryPoint::new(
        METHOD_ADD_BID_PROXY_CALL_1,
        vec![
            Parameter::new(auction::ARG_PUBLIC_KEY, PublicKey::cl_type()),
            Parameter::new(auction::ARG_DELEGATION_RATE, DelegationRate::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        U512::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(add_bid_proxy_call_1);

    let add_bid_proxy_call = EntryPoint::new(
        METHOD_ADD_BID_PROXY_CALL,
        vec![
            Parameter::new(auction::ARG_PUBLIC_KEY, PublicKey::cl_type()),
            Parameter::new(auction::ARG_DELEGATION_RATE, DelegationRate::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        U512::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(add_bid_proxy_call);

    let withdraw_proxy_call_1 = EntryPoint::new(
        METHOD_WITHDRAW_PROXY_CALL_1,
        vec![
            Parameter::new(auction::ARG_PUBLIC_KEY, PublicKey::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    let withdraw_proxy_call = EntryPoint::new(
        METHOD_WITHDRAW_PROXY_CALL,
        vec![
            Parameter::new(auction::ARG_PUBLIC_KEY, PublicKey::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    let delegate_proxy_call = EntryPoint::new(
        METHOD_DELEGATE_PROXY_CALL,
        vec![
            Parameter::new(auction::ARG_DELEGATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_VALIDATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        U512::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    let delegate_proxy_call_1 = EntryPoint::new(
        METHOD_DELEGATE_PROXY_CALL_1,
        vec![
            Parameter::new(auction::ARG_DELEGATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_VALIDATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        U512::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    let undelegate_proxy_call = EntryPoint::new(
        METHOD_UNDELEGATE_PROXY_CALL,
        vec![
            Parameter::new(auction::ARG_DELEGATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_VALIDATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        U512::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    let undelegate_proxy_call_1 = EntryPoint::new(
        METHOD_UNDELEGATE_PROXY_CALL_1,
        vec![
            Parameter::new(auction::ARG_DELEGATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_VALIDATOR, PublicKey::cl_type()),
            Parameter::new(auction::ARG_AMOUNT, U512::cl_type()),
        ],
        U512::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    let activate_bid_proxy_call = EntryPoint::new(
        METHOD_ACTIVATE_BID_CALL,
        vec![Parameter::new(
            auction::ARG_VALIDATOR_PUBLIC_KEY,
            CLType::PublicKey,
        )],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    let activate_bid_proxy_call_1 = EntryPoint::new(
        METHOD_ACTIVATE_BID_CALL_1,
        vec![Parameter::new(
            auction::ARG_VALIDATOR_PUBLIC_KEY,
            CLType::PublicKey,
        )],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );

    entry_points.add_entry_point(withdraw_proxy_call);
    entry_points.add_entry_point(withdraw_proxy_call_1);

    entry_points.add_entry_point(delegate_proxy_call);
    entry_points.add_entry_point(delegate_proxy_call_1);

    entry_points.add_entry_point(undelegate_proxy_call);
    entry_points.add_entry_point(undelegate_proxy_call_1);

    entry_points.add_entry_point(activate_bid_proxy_call);
    entry_points.add_entry_point(activate_bid_proxy_call_1);

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();

    // runtime::put_key(PACKAGE_HASH_NAME, contract_package_hash);
    runtime::put_key(ACCESS_UREF_NAME, access_uref.into());

    let mut named_keys = NamedKeys::new();
    named_keys.insert(PACKAGE_HASH_NAME.to_string(), contract_package_hash.into());

    let (contract_hash, _version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);
    runtime::put_key(CONTRACT_HASH_NAME, contract_hash.into());
}
