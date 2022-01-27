#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use casper_contract::ext_ffi;
use casper_types::{api_error, ApiError, UREF_SERIALIZED_LENGTH};

fn custom_create_purse(buffer_size: usize) -> Result<(), ApiError> {
    let big_purse = vec![0u8; buffer_size];
    let ret = unsafe { ext_ffi::casper_create_purse(big_purse.as_ptr(), big_purse.len()) };
    api_error::result_from(ret)
}

#[no_mangle]
pub extern "C" fn call() {
    assert_eq!(custom_create_purse(1024), Ok(()));
    assert_eq!(custom_create_purse(0), Err(ApiError::PurseNotCreated));
    assert_eq!(custom_create_purse(3), Err(ApiError::PurseNotCreated));
    assert_eq!(custom_create_purse(UREF_SERIALIZED_LENGTH), Ok(()));
}
