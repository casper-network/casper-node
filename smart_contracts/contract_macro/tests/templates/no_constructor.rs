#![cfg_attr(
    not(target_arch = "wasm32"),
    crate_type = "target arch should be wasm32"
)]
extern crate alloc;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
};
use contract_macro::{casperlabs_constructor, casperlabs_contract, casperlabs_method};

use contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use types::{
    account::PublicKey,
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints},
    runtime_args, CLType, CLTyped, Group, Key, Parameter, RuntimeArgs, URef, U512,
};

const KEY: &str = "string_value";
const INT_KEY: &str = "int_value";
const U512_KEY: &str = "u512_value";

#[casperlabs_contract]
mod sample_contract {
    use super::*;
    fn store_u64() {
    /// Compilation should fail as there is no constructor
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

fn main() {}
