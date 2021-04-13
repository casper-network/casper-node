#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{ApiError, URef, U512};

const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_NAME: &str = "name";
const ARG_VALUE: &str = "value";

const ARG_RESULT: &str = "result";

const METHOD_WRITE: &str = "write";
const METHOD_DELETE: &str = "delete";
const METHOD_READ: &str = "read";

#[repr(u16)]
enum Error {
    UnknownCommand,
}

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_ENTRY_POINT);

    match command.as_str() {
        METHOD_WRITE => {
            let value_name: String = runtime::get_named_arg(ARG_NAME);
            let value: U512 = runtime::get_named_arg(ARG_VALUE);
            let value_uref: URef = storage::new_uref(value);
            runtime::put_key(&value_name, value_uref.into());
        }
        METHOD_DELETE => {
            let value_name: String = runtime::get_named_arg(ARG_NAME);
            let value_uref: URef = {
                let value_key = runtime::get_key(&value_name).unwrap_or_revert();
                value_key.into_uref().unwrap_or_revert()
            };
            storage::delete(value_uref);
        }
        METHOD_READ => {
            let value_name: String = runtime::get_named_arg(ARG_NAME);
            let result_name: String = runtime::get_named_arg(ARG_RESULT);
            let value_uref: URef = {
                let value_key = runtime::get_key(&value_name).unwrap_or_revert();
                value_key.into_uref().unwrap_or_revert()
            };
            let maybe_value: Option<U512> = storage::read(value_uref).unwrap_or_revert();
            let name_read_uref: URef = storage::new_uref(maybe_value);
            runtime::put_key(&result_name, name_read_uref.into());
        }
        _ => runtime::revert(ApiError::User(Error::UnknownCommand as u16)),
    }
}
