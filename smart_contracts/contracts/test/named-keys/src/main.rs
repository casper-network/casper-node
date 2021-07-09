#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use core::convert::TryInto;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{bytesrepr::ToBytes, ApiError, CLTyped, Key, U512};

fn create_uref<T: CLTyped + ToBytes>(key_name: &str, value: T) {
    let key: Key = storage::new_uref(value).into();
    runtime::put_key(key_name, key);
}

const COMMAND_CREATE_UREF1: &str = "create-uref1";
const COMMAND_CREATE_UREF2: &str = "create-uref2";
const COMMAND_REMOVE_UREF1: &str = "remove-uref1";
const COMMAND_REMOVE_UREF2: &str = "remove-uref2";
const COMMAND_TEST_READ_UREF1: &str = "test-read-uref1";
const COMMAND_TEST_READ_UREF2: &str = "test-read-uref2";
const COMMAND_INCREASE_UREF2: &str = "increase-uref2";
const COMMAND_OVERWRITE_UREF2: &str = "overwrite-uref2";
const ARG_COMMAND: &str = "command";

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_COMMAND);

    match command.as_str() {
        COMMAND_CREATE_UREF1 => create_uref("hello-world", String::from("Hello, world!")),
        COMMAND_CREATE_UREF2 => create_uref("big-value", U512::max_value()),
        COMMAND_REMOVE_UREF1 => runtime::remove_key("hello-world"),
        COMMAND_REMOVE_UREF2 => runtime::remove_key("big-value"),
        COMMAND_TEST_READ_UREF1 => {
            // Read data hidden behind `URef1` uref
            let hello_world: String = storage::read(
                (*runtime::list_named_keys()
                    .get("hello-world")
                    .expect("Unable to get hello-world"))
                .try_into()
                .expect("Unable to convert to uref"),
            )
            .expect("Unable to deserialize URef")
            .expect("Unable to find value");
            assert_eq!(hello_world, "Hello, world!");

            // Read data through dedicated FFI function
            let uref1 = runtime::get_key("hello-world").unwrap_or_revert();

            let uref = uref1.try_into().unwrap_or_revert_with(ApiError::User(101));
            let hello_world = storage::read(uref);
            assert_eq!(hello_world, Ok(Some("Hello, world!".to_string())));
        }
        COMMAND_TEST_READ_UREF2 => {
            // Get the big value back
            let big_value_key =
                runtime::get_key("big-value").unwrap_or_revert_with(ApiError::User(102));
            let big_value_ref = big_value_key.try_into().unwrap_or_revert();
            let big_value = storage::read(big_value_ref);
            assert_eq!(big_value, Ok(Some(U512::max_value())));
        }
        COMMAND_INCREASE_UREF2 => {
            // Get the big value back
            let big_value_key =
                runtime::get_key("big-value").unwrap_or_revert_with(ApiError::User(102));
            let big_value_ref = big_value_key.try_into().unwrap_or_revert();
            // Increase by 1
            storage::add(big_value_ref, U512::one());
            let new_big_value = storage::read(big_value_ref);
            assert_eq!(new_big_value, Ok(Some(U512::zero())));
        }
        COMMAND_OVERWRITE_UREF2 => {
            // Get the big value back
            let big_value_key =
                runtime::get_key("big-value").unwrap_or_revert_with(ApiError::User(102));
            let big_value_ref = big_value_key.try_into().unwrap_or_revert();
            // I can overwrite some data under the pointer
            storage::write(big_value_ref, U512::from(123_456_789u64));
            let new_value = storage::read(big_value_ref);
            assert_eq!(new_value, Ok(Some(U512::from(123_456_789u64))));
        }
        _ => runtime::revert(ApiError::InvalidArgument),
    }
}
