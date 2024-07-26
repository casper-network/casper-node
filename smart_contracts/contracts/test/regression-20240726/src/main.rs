#![no_std]
#![no_main]

extern crate alloc;

use alloc::{collections::BTreeMap, string::ToString, vec};
use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash, CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    Parameter, PublicKey, URef, U512,
};

#[no_mangle]
pub extern "C" fn withdraw() {
    let purse = runtime::get_key("seed")
        .unwrap_or_revert()
        .into_uref()
        .unwrap_or_revert();
    let target: AccountHash = runtime::get_named_arg::<PublicKey>("target").to_account_hash();
    let amount = U512::from(2_500_000_000u64);

    system::transfer_from_purse_to_account(purse, target, amount, None).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn call() {
    let uref_str = "uref-bbd996744e771ce1b2af10104d6bb596576c4e1a797ac8bbdd273cbb5dc695e6-007";
    let purse: Key = URef::from_formatted_str(uref_str).unwrap().into();

    let entry_points = {
        let mut ret = EntryPoints::new();
        let entry_point = EntryPoint::new(
            "withdraw",
            vec![Parameter::new("target", CLType::PublicKey)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        ret.add_entry_point(entry_point);
        ret
    };

    let named_keys = {
        let mut ret = BTreeMap::new();
        ret.insert("seed".to_string(), purse);
        ret
    };

    let (contract_hash, _) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some("package-hash".to_string()),
        Some("access-uref".to_string()),
    );
    runtime::put_key("contract-hash", Key::Hash(contract_hash.value()))
}
