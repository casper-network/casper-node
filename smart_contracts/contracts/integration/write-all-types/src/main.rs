#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::{collections::BTreeMap, string::String, vec::Vec};
use casperlabs_contract::contract_api::{runtime, storage};
use casperlabs_types::{
    contracts::Parameters, CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    URef, U128, U256, U512,
};

#[no_mangle]
pub extern "C" fn do_nothing() {}

#[no_mangle]
#[allow(clippy::let_unit_value, clippy::unit_arg)]
pub extern "C" fn call() {
    let v01: bool = true;
    let v02: i32 = 2;
    let v03: i64 = 3;
    let v04: u8 = 4;
    let v05: u32 = 5;
    let v06: u64 = 6;
    let v07: U128 = U128::from(7);
    let v08: U256 = U256::from(8);
    let v09: U512 = U512::from(9);
    let v10: () = ();
    let v11: String = String::from("Hello, World");
    let v12: Key = Key::Hash([12u8; 32]);
    let v13: URef = storage::new_uref(());
    let v14: Option<u32> = Some(14);
    let v15: Vec<String> = vec![String::from("ABCD"), String::from("EFG")];
    let v16: [Option<u8>; 4] = [None, Some(0), Some(1), None];
    let v17: Result<U512, String> = Ok(U512::from(17));
    let v18: BTreeMap<i32, bool> = vec![(0, false), (1, true), (3, true)].into_iter().collect();
    let v19: (u64,) = (19,);
    let v20: (u8, u32) = (0, 1);
    let v21: (u8, u32, u64) = (0, 1, 2);

    let u001 = storage::new_uref(v01);
    let u002 = storage::new_uref(v02);
    let u003 = storage::new_uref(v03);
    let u004 = storage::new_uref(v04);
    let u005 = storage::new_uref(v05);
    let u006 = storage::new_uref(v06);
    let u007 = storage::new_uref(v07);
    let u008 = storage::new_uref(v08);
    let u009 = storage::new_uref(v09);
    let u010 = storage::new_uref(v10);
    let u011 = storage::new_uref(v11);
    let u012 = storage::new_uref(v12);
    let u013 = storage::new_uref(v13);
    let u014 = storage::new_uref(v14);
    let u015 = storage::new_uref(v15);
    let u016 = storage::new_uref(v16);
    let u017 = storage::new_uref(v17);
    let u018 = storage::new_uref(v18);
    let u019 = storage::new_uref(v19);
    let u020 = storage::new_uref(v20);
    let u021 = storage::new_uref(v21);
    let u022 = {
        let mut entry_points = EntryPoints::new();
        entry_points.add_entry_point(EntryPoint::new(
            "do_nothing",
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        ));
        let (contract_hash, _contract_version) =
            storage::new_contract(entry_points, None, None, None);
        contract_hash
    };

    runtime::put_key("v01", u001.into());
    runtime::put_key("v02", u002.into());
    runtime::put_key("v03", u003.into());
    runtime::put_key("v04", u004.into());
    runtime::put_key("v05", u005.into());
    runtime::put_key("v06", u006.into());
    runtime::put_key("v07", u007.into());
    runtime::put_key("v08", u008.into());
    runtime::put_key("v09", u009.into());
    runtime::put_key("v10", u010.into());
    runtime::put_key("v11", u011.into());
    runtime::put_key("v12", u012.into());
    runtime::put_key("v13", u013.into());
    runtime::put_key("v14", u014.into());
    runtime::put_key("v15", u015.into());
    runtime::put_key("v16", u016.into());
    runtime::put_key("v17", u017.into());
    runtime::put_key("v18", u018.into());
    runtime::put_key("v19", u019.into());
    runtime::put_key("v20", u020.into());
    runtime::put_key("v21", u021.into());
    runtime::put_key("v22", u022.into());
}
