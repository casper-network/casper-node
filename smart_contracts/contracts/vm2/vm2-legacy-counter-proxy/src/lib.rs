#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_macros::{casper, PanicOnDefault};
use casper_sdk::{host, log, types::Address};

/// This contract implements a simple LegacyCounterProxy.
#[derive(PanicOnDefault)]
#[casper(contract_state)]
pub struct LegacyCounterProxy {
    /// Legacy address of the counter contract.
    legacy_address: Address,
}

const EMPTY_RUNTIME_ARGS: [u8; 4] = 0u32.to_le_bytes();
const CL_VALUE_UNIT_BYTES: [u8; 5] = [0, 0, 0, 0, 9];

#[casper]
impl LegacyCounterProxy {
    #[casper(constructor)]
    pub fn new(legacy_address: Address) -> Self {
        Self { legacy_address }
    }

    pub fn perform_test(&self) {
        let (counter_get_result_before, host_error) =
            host::casper_call(&self.legacy_address, 0, "counter_get", &EMPTY_RUNTIME_ARGS);
        log!("counter_get_result_before: {:?}", counter_get_result_before);
        // assert_eq!(counter_get_result_before, 0);
        let _ = host_error.expect("No error 1");

        let (inc_result, host_error) =
            host::casper_call(&self.legacy_address, 0, "counter_inc", &EMPTY_RUNTIME_ARGS);
        log!("inc_result {:?}", inc_result);
        assert_eq!(inc_result, Some(CL_VALUE_UNIT_BYTES.to_vec()));
        let _ = host_error.expect("No error 2");

        let (counter_get_result_after, host_error) =
            host::casper_call(&self.legacy_address, 0, "counter_get", &EMPTY_RUNTIME_ARGS);
        let _ = host_error.expect("No error 3");
        log!("counter_get_result_after: {:?}", counter_get_result_after);
        assert_ne!(counter_get_result_before, counter_get_result_after);
    }
}
