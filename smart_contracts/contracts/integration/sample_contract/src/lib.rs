extern crate alloc;
use alloc::{collections::BTreeSet, string::String};
use contract_macro::{casperlabs_constructor, casperlabs_contract, casperlabs_method};

use contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use types::{
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints},
    runtime_args, CLType, Group, Key, Parameter, RuntimeArgs, URef, U512,
};

const KEY: &str = "string_value";
const INT_KEY: &str = "int_value";
const U512_KEY: &str = "u512_value";

#[casperlabs_contract]
mod sample_contract {
    use super::*;
    #[casperlabs_constructor]
    fn store_hello_world(s: String, _a: u64) {
        let value_ref: URef = storage::new_uref(s);
        let value_key: Key = value_ref.into();
        runtime::put_key(KEY, value_key);
    }
    #[casperlabs_method]
    fn store_u64() {
        let u: u64 = bar();
        let int_ref: URef = storage::new_uref(u);
        let int_key: Key = int_ref.into();
        runtime::put_key(INT_KEY, int_key)
    }

    #[casperlabs_method]
    fn store_u512(c: U512) {
        let int_ref: URef = storage::new_uref(c);
        let int_key: Key = int_ref.into();
        runtime::put_key(U512_KEY, int_key)
    }

    fn bar() -> u64 {
        1
    }
}
