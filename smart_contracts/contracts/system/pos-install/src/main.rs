#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    vec,
};

use casperlabs_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::AccountHash,
    contracts::{
        EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys, Parameter,
        CONTRACT_INITIAL_VERSION,
    },
    proof_of_stake::Stakes,
    runtime_args,
    system_contract_errors::mint,
    CLType, CLValue, ContractPackageHash, Key, RuntimeArgs, URef, U512,
};
use pos::{
    ARG_ACCOUNT_KEY, ARG_AMOUNT, ARG_PURSE, METHOD_BOND, METHOD_FINALIZE_PAYMENT,
    METHOD_GET_PAYMENT_PURSE, METHOD_GET_REFUND_PURSE, METHOD_SET_REFUND_PURSE, METHOD_UNBOND,
};

const PLACEHOLDER_KEY: Key = Key::Hash([0u8; 32]);
const POS_BONDING_PURSE: &str = "pos_bonding_purse";
const POS_PAYMENT_PURSE: &str = "pos_payment_purse";
const POS_REWARDS_PURSE: &str = "pos_rewards_purse";

const ARG_MINT_PACKAGE_HASH: &str = "mint_contract_package_hash";
const ARG_GENESIS_VALIDATORS: &str = "genesis_validators";
const ENTRY_POINT_MINT: &str = "mint";

const HASH_KEY_NAME: &str = "pos_hash";
const ACCESS_KEY_NAME: &str = "pos_access";

#[no_mangle]
pub extern "C" fn bond() {
    pos::bond();
}

#[no_mangle]
pub extern "C" fn unbond() {
    pos::unbond();
}

#[no_mangle]
pub extern "C" fn get_payment_purse() {
    pos::get_payment_purse();
}

#[no_mangle]
pub extern "C" fn set_refund_purse() {
    pos::set_refund_purse();
}

#[no_mangle]
pub extern "C" fn get_refund_purse() {
    pos::get_refund_purse();
}

#[no_mangle]
pub extern "C" fn finalize_payment() {
    pos::finalize_payment();
}

#[no_mangle]
pub extern "C" fn install() {
    let mint_package_hash: ContractPackageHash = runtime::get_named_arg(ARG_MINT_PACKAGE_HASH);
    let genesis_validators: BTreeMap<AccountHash, U512> =
        runtime::get_named_arg(ARG_GENESIS_VALIDATORS);

    let stakes = Stakes::new(genesis_validators);

    // Add genesis validators to PoS contract object.
    // For now, we are storing validators in `named_keys` map of the PoS contract
    // in the form: key: "v_{validator_pk}_{validator_stake}", value: doesn't
    // matter.
    let mut named_keys: NamedKeys = stakes.strings().map(|key| (key, PLACEHOLDER_KEY)).collect();
    let total_bonds: U512 = stakes.total_bonds();
    let bonding_purse = mint_purse(mint_package_hash, total_bonds);
    let payment_purse = mint_purse(mint_package_hash, U512::zero());
    let rewards_purse = mint_purse(mint_package_hash, U512::zero());

    // Include PoS purses in its named_keys
    [
        (POS_BONDING_PURSE, bonding_purse),
        (POS_PAYMENT_PURSE, payment_purse),
        (POS_REWARDS_PURSE, rewards_purse),
    ]
    .iter()
    .for_each(|(name, uref)| {
        named_keys.insert(String::from(*name), Key::URef(*uref));
    });

    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let bond = EntryPoint::new(
            METHOD_BOND.to_string(),
            vec![
                Parameter::new(ARG_AMOUNT, CLType::U512),
                Parameter::new(ARG_PURSE, CLType::URef),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(bond);

        let unbond = EntryPoint::new(
            METHOD_UNBOND.to_string(),
            vec![Parameter::new(ARG_AMOUNT, CLType::U512)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(unbond);

        let get_payment_purse = EntryPoint::new(
            METHOD_GET_PAYMENT_PURSE.to_string(),
            vec![],
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(get_payment_purse);

        let set_refund_purse = EntryPoint::new(
            METHOD_SET_REFUND_PURSE.to_string(),
            vec![Parameter::new(ARG_PURSE, CLType::URef)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(set_refund_purse);

        let get_refund_purse = EntryPoint::new(
            METHOD_GET_REFUND_PURSE.to_string(),
            vec![],
            CLType::Option(Box::new(CLType::URef)),
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(get_refund_purse);

        let finalize_payment = EntryPoint::new(
            METHOD_FINALIZE_PAYMENT.to_string(),
            vec![
                Parameter::new(ARG_AMOUNT, CLType::U512),
                Parameter::new(ARG_ACCOUNT_KEY, CLType::FixedList(Box::new(CLType::U8), 32)),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(finalize_payment);

        entry_points
    };

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t((contract_package_hash, contract_key)).unwrap_or_revert();
    runtime::ret(return_value);
}

fn mint_purse(contract_package_hash: ContractPackageHash, amount: U512) -> URef {
    let args = runtime_args! {
        ARG_AMOUNT => amount,
    };

    let result: Result<URef, mint::Error> = runtime::call_versioned_contract(
        contract_package_hash,
        Some(CONTRACT_INITIAL_VERSION),
        ENTRY_POINT_MINT,
        args,
    );

    result.unwrap_or_revert()
}
