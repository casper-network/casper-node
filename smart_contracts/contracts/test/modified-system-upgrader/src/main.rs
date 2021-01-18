#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;

use alloc::collections::BTreeMap;
use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::{NamedKeys, Parameters},
    mint::{ACCESS_KEY, HASH_KEY},
    proof_of_stake::{
        ARG_ACCOUNT, ARG_AMOUNT, ARG_PURSE, METHOD_FINALIZE_PAYMENT, METHOD_GET_PAYMENT_PURSE,
        METHOD_GET_REFUND_PURSE, METHOD_SET_REFUND_PURSE,
    },
    standard_payment::METHOD_PAY,
    CLType, CLValue, ContractHash, ContractPackageHash, ContractVersion, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints, Parameter, URef,
};

pub const MODIFIED_MINT_EXT_FUNCTION_NAME: &str = "modified_mint_ext";
pub const POS_EXT_FUNCTION_NAME: &str = "pos_ext";
const VERSION_ENTRY_POINT: &str = "version";
const UPGRADED_VERSION: &str = "1.1.0";

#[no_mangle]
pub extern "C" fn mint() {
    modified_mint::mint();
}

#[no_mangle]
pub extern "C" fn create() {
    modified_mint::create();
}

#[no_mangle]
pub extern "C" fn balance() {
    modified_mint::balance();
}

#[no_mangle]
pub extern "C" fn transfer() {
    modified_mint::transfer();
}

#[no_mangle]
pub extern "C" fn read_base_round_reward() {
    modified_mint::read_base_round_reward()
}

#[no_mangle]
pub extern "C" fn version() {
    runtime::ret(CLValue::from_t(UPGRADED_VERSION).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn pay() {
    standard_payment::delegate();
}

fn upgrade_mint() -> (ContractHash, ContractVersion) {
    let mint_package_hash: ContractPackageHash = runtime::get_key(HASH_KEY)
        .expect("should have mint")
        .into_hash()
        .expect("should be hash")
        .into();
    let _mint_access_key: URef = runtime::get_key(ACCESS_KEY)
        .unwrap_or_revert()
        .into_uref()
        .expect("should be uref");

    let mut entry_points = modified_mint::get_entry_points();
    let entry_point = EntryPoint::new(
        VERSION_ENTRY_POINT,
        Parameters::new(),
        CLType::String,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let named_keys = NamedKeys::new();
    storage::add_contract_version(mint_package_hash, entry_points, named_keys)
}

fn upgrade_proof_of_stake() -> (ContractHash, ContractVersion) {
    const HASH_KEY_NAME: &str = "pos_hash";
    const ACCESS_KEY_NAME: &str = "pos_access";

    let pos_package_hash: ContractPackageHash = runtime::get_key(HASH_KEY_NAME)
        .expect("should have mint")
        .into_hash()
        .expect("should be hash")
        .into();
    let _pos_access_key: URef = runtime::get_key(ACCESS_KEY_NAME)
        .unwrap_or_revert()
        .into_uref()
        .expect("should be uref");

    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let get_payment_purse = EntryPoint::new(
            METHOD_GET_PAYMENT_PURSE,
            vec![],
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(get_payment_purse);

        let set_refund_purse = EntryPoint::new(
            METHOD_SET_REFUND_PURSE,
            vec![Parameter::new(ARG_PURSE, CLType::URef)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(set_refund_purse);

        let get_refund_purse = EntryPoint::new(
            METHOD_GET_REFUND_PURSE,
            vec![],
            CLType::Option(Box::new(CLType::URef)),
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(get_refund_purse);

        let finalize_payment = EntryPoint::new(
            METHOD_FINALIZE_PAYMENT,
            vec![
                Parameter::new(ARG_AMOUNT, CLType::U512),
                Parameter::new(ARG_ACCOUNT, CLType::ByteArray(32)),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(finalize_payment);

        entry_points
    };

    let named_keys = NamedKeys::new();

    storage::add_contract_version(pos_package_hash, entry_points, named_keys)
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

fn upgrade_standard_payment() -> (ContractHash, ContractVersion) {
    const HASH_KEY_NAME: &str = "standard_payment_hash";
    const ACCESS_KEY_NAME: &str = "standard_payment_access";
    const ARG_AMOUNT: &str = "amount";

    let standard_payment_package_hash: ContractPackageHash = runtime::get_key(HASH_KEY_NAME)
        .expect("should have mint")
        .into_hash()
        .expect("should be hash")
        .into();
    let _standard_payment_access_key: URef = runtime::get_key(ACCESS_KEY_NAME)
        .unwrap_or_revert()
        .into_uref()
        .expect("shuold be uref");

    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            METHOD_PAY,
            vec![Parameter::new(ARG_AMOUNT, CLType::U512)],
            CLType::Result {
                ok: Box::new(CLType::Unit),
                err: Box::new(CLType::U32),
            },
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        entry_points.add_entry_point(entry_point);

        entry_points
    };

    let named_keys = NamedKeys::new();

    storage::add_contract_version(standard_payment_package_hash, entry_points, named_keys)
}

#[no_mangle]
pub extern "C" fn upgrade() {
    let mut upgrades: BTreeMap<ContractHash, ContractHash> = BTreeMap::new();

    {
        let old_mint_hash = system::get_mint();
        let (new_mint_hash, _new_mint_version) = upgrade_mint();
        upgrades.insert(old_mint_hash, new_mint_hash);
    }

    {
        let old_pos_hash = system::get_proof_of_stake();
        let (new_pos_hash, _new_pos_version) = upgrade_proof_of_stake();
        upgrades.insert(old_pos_hash, new_pos_hash);
    }

    {
        let old_standard_payment_hash = system::get_standard_payment();
        let (new_standard_payment_hash, _new_standard_payment_version) = upgrade_standard_payment();
        upgrades.insert(old_standard_payment_hash, new_standard_payment_hash);
    }

    runtime::ret(CLValue::from_t(upgrades).unwrap());
}
