//! Some functions to use in tests.

use casperlabs_types::{account::AccountHash, contracts::NamedKeys, AccessRights, Key, URef};

use crate::components::contract_runtime::shared::{
    account::Account, stored_value::StoredValue, wasm_costs::WasmCosts,
};

/// Returns an account value paired with its key
pub fn mocked_account(account_hash: AccountHash) -> Vec<(Key, StoredValue)> {
    let purse = URef::new([0u8; 32], AccessRights::READ_ADD_WRITE);
    let account = Account::create(account_hash, NamedKeys::new(), purse);
    vec![(Key::Account(account_hash), StoredValue::Account(account))]
}

pub fn wasm_costs_mock() -> WasmCosts {
    WasmCosts {
        regular: 1,
        div: 16,
        mul: 4,
        mem: 2,
        initial_mem: 4096,
        grow_mem: 8192,
        memcpy: 1,
        max_stack_height: 64 * 1024,
        opcodes_mul: 3,
        opcodes_div: 8,
    }
}

pub fn wasm_costs_free() -> WasmCosts {
    WasmCosts {
        regular: 0,
        div: 0,
        mul: 0,
        mem: 0,
        initial_mem: 4096,
        grow_mem: 8192,
        memcpy: 0,
        max_stack_height: 64 * 1024,
        opcodes_mul: 1,
        opcodes_div: 1,
    }
}
