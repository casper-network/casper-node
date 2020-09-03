#![no_std]
#![no_main]

use casper_contract::contract_api::runtime;
use casper_types::Phase;

const ARG_PHASE: &str = "phase";

#[no_mangle]
pub extern "C" fn call() {
    let known_phase: Phase = runtime::get_named_arg(ARG_PHASE);
    let get_phase = runtime::get_phase();
    assert_eq!(
        get_phase, known_phase,
        "get_phase did not return known_phase"
    );
}
