#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage};
use casper_types::Phase;

const RANDOM_BYTES_RESULT: &str = "random_bytes_result";

#[no_mangle]
pub extern "C" fn call() {
    let get_phase = runtime::get_phase();
    assert_ne!(
        Phase::Payment,
        get_phase,
        "should not be invoked in payment phase"
    );

    let random_bytes = runtime::random_bytes();
    let uref = storage::new_uref(random_bytes);
    runtime::put_key(RANDOM_BYTES_RESULT, uref.into())
}
