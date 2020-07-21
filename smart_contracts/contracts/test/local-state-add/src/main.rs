#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casperlabs_contract::contract_api::{runtime, storage};

pub const LOCAL_KEY: [u8; 32] = [66u8; 32];

const CMD_WRITE: &str = "write";
const CMD_ADD: &str = "add";

const INITIAL_VALUE: u64 = 10;
const ADD_VALUE: u64 = 5;

const ARG_COMMAND: &str = "command";

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_COMMAND);

    if command == CMD_WRITE {
        storage::write_local(LOCAL_KEY, INITIAL_VALUE);
    } else if command == CMD_ADD {
        storage::add_local(LOCAL_KEY, ADD_VALUE);
    }
}
