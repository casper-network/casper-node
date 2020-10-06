//! Some functions to use in tests.

use casper_types::{account::AccountHash, contracts::NamedKeys, AccessRights, Key, URef};

use crate::shared::{account::Account, opcode_costs::OpCodeCosts, stored_value::StoredValue};

/// Returns an account value paired with its key
pub fn mocked_account(account_hash: AccountHash) -> Vec<(Key, StoredValue)> {
    let purse = URef::new([0u8; 32], AccessRights::READ_ADD_WRITE);
    let account = Account::create(account_hash, NamedKeys::new(), purse);
    vec![(Key::Account(account_hash), StoredValue::Account(account))]
}

pub fn wasm_costs_mock() -> OpCodeCosts {
    OpCodeCosts {
        regular: 1,
        div: 16,
        mul: 4,
        mem: 2,
        grow_mem: 8192,
    }
}

pub fn wasm_costs_free() -> OpCodeCosts {
    OpCodeCosts {
        regular: 0,
        div: 0,
        mul: 0,
        mem: 0,
        grow_mem: 8192,
    }
}
