#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::{prelude::*, types::Address};

/// This contract implements a simple Vm1Wrapper.
#[derive(PanicOnDefault)]
#[casper(contract_state)]
pub struct Vm1Wrapper {
    /// Address of the VM1 counter contract.
    contract_address: Address,
}

const EMPTY_RUNTIME_ARGS: [u8; 4] = 0u32.to_le_bytes();
const CL_VALUE_UNIT_BYTES: [u8; 5] = [0, 0, 0, 0, 9];

#[casper]
impl Vm1Wrapper {
    #[casper(constructor)]
    pub fn new(contract_address: Address) -> Self {
        Self { contract_address }
    }

    pub fn perform_test(&self) {
        let (counter_get_result_1, host_error) = casper::casper_call(
            &self.contract_address,
            0,
            "counter_get",
            &EMPTY_RUNTIME_ARGS,
        );
        log!("counter_get_result_before: {:?}", counter_get_result_1);
        let _ = host_error.expect("No error 1");

        let (inc_result_1, host_error) = casper::casper_call(
            &self.contract_address,
            0,
            "counter_inc",
            &EMPTY_RUNTIME_ARGS,
        );
        log!("inc_result {:?}", inc_result_1);
        assert_eq!(inc_result_1, Some(CL_VALUE_UNIT_BYTES.to_vec()));
        let _ = host_error.expect("No error 2");

        let (counter_get_result_2, host_error) = casper::casper_call(
            &self.contract_address,
            0,
            "counter_get",
            &EMPTY_RUNTIME_ARGS,
        );
        let _ = host_error.expect("No error 3");
        log!("counter_get_result_after: {:?}", counter_get_result_2);
        assert_ne!(counter_get_result_1, counter_get_result_2);

        let (inc_result_2, host_error) = casper::casper_call(
            &self.contract_address,
            0,
            "counter_inc",
            &EMPTY_RUNTIME_ARGS,
        );
        log!("inc_result {:?}", inc_result_2);
        assert_eq!(inc_result_2, Some(CL_VALUE_UNIT_BYTES.to_vec()));
        let _ = host_error.expect("No error 4");

        let (counter_get_result_3, host_error) = casper::casper_call(
            &self.contract_address,
            0,
            "counter_get",
            &EMPTY_RUNTIME_ARGS,
        );
        let _ = host_error.expect("No error 3");
        log!("counter_get_result_after: {:?}", counter_get_result_3);
        assert_ne!(counter_get_result_2, counter_get_result_3);
    }
}
