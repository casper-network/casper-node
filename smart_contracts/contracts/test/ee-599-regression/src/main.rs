#![no_std]
#![no_main]
#![allow(unused_imports)]

extern crate alloc;

use alloc::{collections::BTreeMap, string::String};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    contracts::{NamedKeys, Parameters},
    ApiError, CLType, ContractHash, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    RuntimeArgs, URef, U512,
};

const DONATION_AMOUNT: u64 = 1;
// Different name just to make sure any routine that deals with named keys coming from different
// sources wouldn't overlap (if ever that's possible)
const DONATION_PURSE_COPY: &str = "donation_purse_copy";
const DONATION_PURSE: &str = "donation_purse";
const MAINTAINER: &str = "maintainer";
const METHOD_CALL: &str = "call";
const METHOD_INSTALL: &str = "install";
const TRANSFER_FROM_PURSE_TO_ACCOUNT: &str = "transfer_from_purse_to_account_ext";
const TRANSFER_FROM_PURSE_TO_PURSE: &str = "transfer_from_purse_to_purse_ext";
const TRANSFER_FUNDS_KEY: &str = "transfer_funds";
const TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_ext";

const ARG_METHOD: &str = "method";
const ARG_CONTRACTKEY: &str = "contract_key";
const ARG_SUBCONTRACTMETHODFWD: &str = "sub_contract_method_fwd";

#[repr(u16)]
enum ContractError {
    InvalidDelegateMethod = 0,
}

impl From<ContractError> for ApiError {
    fn from(error: ContractError) -> Self {
        ApiError::User(error as u16)
    }
}

fn get_maintainer_account_hash() -> Result<AccountHash, ApiError> {
    // Obtain maintainer address from the contract's named keys
    let maintainer_key = runtime::get_key(MAINTAINER).ok_or(ApiError::GetKey)?;
    maintainer_key
        .into_account()
        .ok_or(ApiError::UnexpectedKeyVariant)
}

fn get_donation_purse() -> Result<URef, ApiError> {
    let donation_key = runtime::get_key(DONATION_PURSE).ok_or(ApiError::GetKey)?;
    donation_key
        .into_uref()
        .ok_or(ApiError::UnexpectedKeyVariant)
}

#[no_mangle]
pub extern "C" fn transfer_from_purse_to_purse_ext() {
    // Donation box is the purse funds will be transferred into
    let donation_purse = get_donation_purse().unwrap_or_revert();

    let main_purse = account::get_main_purse();

    system::transfer_from_purse_to_purse(
        main_purse,
        donation_purse,
        U512::from(DONATION_AMOUNT),
        None,
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn get_main_purse_ext() {}

#[no_mangle]
pub extern "C" fn transfer_from_purse_to_account_ext() {
    let main_purse = account::get_main_purse();
    // This is the address of account which installed the contract
    let maintainer_account_hash = get_maintainer_account_hash().unwrap_or_revert();
    system::transfer_from_purse_to_account(
        main_purse,
        maintainer_account_hash,
        U512::from(DONATION_AMOUNT),
        None,
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn transfer_to_account_ext() {
    // This is the address of account which installed the contract
    let maintainer_account_hash = get_maintainer_account_hash().unwrap_or_revert();
    system::transfer_to_account(maintainer_account_hash, U512::from(DONATION_AMOUNT), None)
        .unwrap_or_revert();
    let _main_purse = account::get_main_purse();
}

/// Registers a function and saves it in callers named keys
fn delegate() -> Result<(), ApiError> {
    let method: String = runtime::get_named_arg(ARG_METHOD);
    match method.as_str() {
        METHOD_INSTALL => {
            // Create a purse that should be known to the contract regardless of the
            // calling context still owned by the account that deploys the contract
            let purse = system::create_purse();
            let maintainer = runtime::get_caller();
            // Keys below will make it possible to use within the called contract
            let known_keys: NamedKeys = {
                let mut keys = NamedKeys::new();
                // "donation_purse" is the purse owner of the contract can transfer funds from
                // callers
                keys.insert(DONATION_PURSE.into(), purse.into());
                // "maintainer" is the person who installed this contract
                keys.insert(MAINTAINER.into(), Key::Account(maintainer));
                keys
            };
            // Install the contract with associated owner-related keys
            // let contract_ref = storage::store_function_at_hash(TRANSFER_FUNDS_EXT, known_keys);

            let entry_points = {
                let mut entry_points = EntryPoints::new();

                let entry_point_1 = EntryPoint::new(
                    TRANSFER_FROM_PURSE_TO_ACCOUNT,
                    Parameters::default(),
                    CLType::Unit,
                    EntryPointAccess::Public,
                    EntryPointType::Contract,
                );

                entry_points.add_entry_point(entry_point_1);

                let entry_point_2 = EntryPoint::new(
                    TRANSFER_TO_ACCOUNT,
                    Parameters::default(),
                    CLType::Unit,
                    EntryPointAccess::Public,
                    EntryPointType::Contract,
                );

                entry_points.add_entry_point(entry_point_2);

                let entry_point_3 = EntryPoint::new(
                    TRANSFER_TO_ACCOUNT,
                    Parameters::default(),
                    CLType::Unit,
                    EntryPointAccess::Public,
                    EntryPointType::Contract,
                );

                entry_points.add_entry_point(entry_point_3);

                let entry_point_4 = EntryPoint::new(
                    TRANSFER_FROM_PURSE_TO_PURSE,
                    Parameters::default(),
                    CLType::Unit,
                    EntryPointAccess::Public,
                    EntryPointType::Contract,
                );

                entry_points.add_entry_point(entry_point_4);

                entry_points
            };

            let (contract_hash, _contract_version) =
                storage::new_contract(entry_points, Some(known_keys), None, None);
            runtime::put_key(TRANSFER_FUNDS_KEY, contract_hash.into());
            // For easy access in outside world here `donation` purse is also attached
            // to the account
            runtime::put_key(DONATION_PURSE_COPY, purse.into());
        }
        METHOD_CALL => {
            // This comes from outside i.e. after deploying the contract, this key is queried,
            // and then passed into the call
            let contract_key: ContractHash = runtime::get_named_arg(ARG_CONTRACTKEY);

            // This is a method that's gets forwarded into the sub contract
            let subcontract_method: String = runtime::get_named_arg(ARG_SUBCONTRACTMETHODFWD);
            runtime::call_contract::<()>(contract_key, &subcontract_method, RuntimeArgs::default());
        }
        _ => return Err(ContractError::InvalidDelegateMethod.into()),
    }
    Ok(())
}

#[no_mangle]
pub extern "C" fn call() {
    delegate().unwrap_or_revert();
}
