// This test focuses on testing whether we charge for
// WASM files that are malformed (unparseable).

// If we're provided with malformed file, we should charge.
// The exception is the "empty wasm" when send as
// a payment, because in such case we use the "default payment"
// instead.

// For increased security, we also test some other cases in this test
// like gas overflow (which is a runtime error).

// Other potential test cases:
// 1. Wasm with unsupported "start" section - tested in `ee_890` (but without asserting the
// charge)

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::MAX_PAYMENT;
use casper_types::{runtime_args, Gas, RuntimeArgs};
use num_traits::Zero;

use crate::{
    test::regression::test_utils::{
        make_gas_counter_overflow, make_module_with_start_section,
        make_module_without_memory_section,
    },
    wasm_utils,
};

const ARG_AMOUNT: &str = "amount";

#[derive(Copy, Clone, Debug)]
enum ExecutionPhase {
    Payment,
    Session,
}

fn run_test_case(input_wasm_bytes: &[u8], expected_error: &str, execution_phase: ExecutionPhase) {
    let payment_amount = *DEFAULT_PAYMENT;

    let (do_minimum_request_builder, expected_error_message) = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash = [42; 32];

        let (deploy_item_builder, expected_error_message) = match execution_phase {
            ExecutionPhase::Payment => (
                DeployItemBuilder::new()
                    .with_payment_bytes(
                        input_wasm_bytes.to_vec(),
                        runtime_args! {ARG_AMOUNT => payment_amount,},
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), session_args),
                expected_error,
            ),
            ExecutionPhase::Session => (
                DeployItemBuilder::new()
                    .with_session_bytes(input_wasm_bytes.to_vec(), session_args)
                    .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => payment_amount,}),
                expected_error,
            ),
        };
        let deploy = deploy_item_builder
            .with_address(account_hash)
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        (
            ExecuteRequestBuilder::new().push_deploy(deploy),
            expected_error_message,
        )
    };
    let do_minimum_request = do_minimum_request_builder.build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .unwrap();

    let proposer_balance_before = builder.get_proposer_purse_balance();

    let account_balance_before = builder.get_purse_balance(account.main_purse());

    let empty_wasm_in_payment = match execution_phase {
        ExecutionPhase::Payment => input_wasm_bytes.is_empty(),
        ExecutionPhase::Session => false,
    };

    if empty_wasm_in_payment {
        // Special case: We expect success, since default payment will be used instead.
        builder.exec(do_minimum_request).expect_success().commit();
    } else {
        builder.exec(do_minimum_request).expect_failure().commit();

        let actual_error = builder.get_error().expect("should have error").to_string();
        assert!(actual_error.contains(expected_error_message));

        let gas = builder.last_exec_gas_cost();
        assert_eq!(gas, Gas::zero());

        let account_balance_after = builder.get_purse_balance(account.main_purse());
        let proposer_balance_after = builder.get_proposer_purse_balance();

        assert_eq!(account_balance_before - *MAX_PAYMENT, account_balance_after);
        assert_eq!(
            proposer_balance_before + *MAX_PAYMENT,
            proposer_balance_after
        );
    }
}

#[ignore]
#[test]
fn should_charge_payment_with_incorrect_wasm_file_invalid_magic_number() {
    const WASM_BYTES: &[u8] = &[1, 2, 3, 4, 5]; // Correct WASM magic bytes are: 0x00 0x61 0x73 0x6d ("\0asm")
    let execution_phase = ExecutionPhase::Payment;
    let expected_error = " Invalid magic number at start of file";
    run_test_case(WASM_BYTES, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_session_with_incorrect_wasm_file_invalid_magic_number() {
    const WASM_BYTES: &[u8] = &[1, 2, 3, 4, 5]; // Correct WASM magic bytes are: 0x00 0x61 0x73 0x6d ("\0asm")
    let execution_phase = ExecutionPhase::Session;
    let expected_error = "Invalid magic number at start of file";
    run_test_case(WASM_BYTES, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_payment_with_incorrect_wasm_file_empty_bytes() {
    const WASM_BYTES: &[u8] = &[];
    let execution_phase = ExecutionPhase::Payment;
    let expected_error = "I/O Error: UnexpectedEof";
    run_test_case(WASM_BYTES, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_session_with_incorrect_wasm_file_empty_bytes() {
    const WASM_BYTES: &[u8] = &[];
    let execution_phase = ExecutionPhase::Session;
    let expected_error = "I/O Error: UnexpectedEof";
    run_test_case(WASM_BYTES, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_payment_with_incorrect_wasm_correct_magic_number_incomplete_module() {
    const WASM_BYTES: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x35, 0x09, 0x60, 0x02, 0x7F, 0x7F,
        0x01, 0x7F, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x01, 0x7F, 0x60, 0x01, 0x7F, 0x00, 0x60, 0x00,
        0x00, 0x60, 0x01, 0x7F, 0x01, 0x7F, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x00, 0x60, 0x05, 0x7F,
        0x7F, 0x7F, 0x7F, 0x7F, 0x01, 0x7F, 0x60, 0x02, 0x7F, 0x7F, 0x00, 0x60, 0x04, 0x7F, 0x7F,
        0x7F, 0x7F, 0x00, 0x02, 0x50, 0x03, 0x03, 0x65, 0x6E, 0x76, 0x16, 0x63, 0x61, 0x73, 0x70,
        0x65, 0x72, 0x5F, 0x6C, 0x6F, 0x61, 0x64, 0x5F, 0x6E, 0x61, 0x6D, 0x65, 0x64, 0x5F, 0x6B,
        0x65, 0x79, 0x73, 0x00, 0x00, 0x03, 0x65, 0x6E, 0x76, 0x17, 0x63, 0x61, 0x73, 0x70, 0x65,
        0x72, 0x5F, 0x72, 0x65, 0x61, 0x64, 0x5F, 0x68, 0x6F, 0x73, 0x74, 0x5F, 0x62, 0x75, 0x66,
        0x66, 0x65, 0x72, 0x00, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x0D, 0x63, 0x61, 0x73, 0x70, 0x65,
        0x72, 0x5F, 0x72, 0x65, 0x76, 0x65, 0x72, 0x74, 0x00,
    ];
    let execution_phase = ExecutionPhase::Payment;
    let expected_error = "I/O Error: UnexpectedEof";
    run_test_case(WASM_BYTES, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_session_with_incorrect_wasm_correct_magic_number_incomplete_module() {
    const WASM_BYTES: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x35, 0x09, 0x60, 0x02, 0x7F, 0x7F,
        0x01, 0x7F, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x01, 0x7F, 0x60, 0x01, 0x7F, 0x00, 0x60, 0x00,
        0x00, 0x60, 0x01, 0x7F, 0x01, 0x7F, 0x60, 0x03, 0x7F, 0x7F, 0x7F, 0x00, 0x60, 0x05, 0x7F,
        0x7F, 0x7F, 0x7F, 0x7F, 0x01, 0x7F, 0x60, 0x02, 0x7F, 0x7F, 0x00, 0x60, 0x04, 0x7F, 0x7F,
        0x7F, 0x7F, 0x00, 0x02, 0x50, 0x03, 0x03, 0x65, 0x6E, 0x76, 0x16, 0x63, 0x61, 0x73, 0x70,
        0x65, 0x72, 0x5F, 0x6C, 0x6F, 0x61, 0x64, 0x5F, 0x6E, 0x61, 0x6D, 0x65, 0x64, 0x5F, 0x6B,
        0x65, 0x79, 0x73, 0x00, 0x00, 0x03, 0x65, 0x6E, 0x76, 0x17, 0x63, 0x61, 0x73, 0x70, 0x65,
        0x72, 0x5F, 0x72, 0x65, 0x61, 0x64, 0x5F, 0x68, 0x6F, 0x73, 0x74, 0x5F, 0x62, 0x75, 0x66,
        0x66, 0x65, 0x72, 0x00, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x0D, 0x63, 0x61, 0x73, 0x70, 0x65,
        0x72, 0x5F, 0x72, 0x65, 0x76, 0x65, 0x72, 0x74, 0x00,
    ];
    let execution_phase = ExecutionPhase::Session;
    let expected_error = "I/O Error: UnexpectedEof";
    run_test_case(WASM_BYTES, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_payment_with_incorrect_wasm_gas_counter_overflow() {
    let wasm_bytes = make_gas_counter_overflow();
    let execution_phase = ExecutionPhase::Payment;
    let expected_error = "Encountered operation forbidden by gas rules";
    run_test_case(&wasm_bytes, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_session_with_incorrect_wasm_gas_counter_overflow() {
    let wasm_bytes = make_gas_counter_overflow();
    let execution_phase = ExecutionPhase::Session;
    let expected_error = "Encountered operation forbidden by gas rules";
    run_test_case(&wasm_bytes, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_payment_with_incorrect_wasm_no_memory_section() {
    let wasm_bytes = make_module_without_memory_section();
    let execution_phase = ExecutionPhase::Payment;
    let expected_error = "Memory section should exist";
    run_test_case(&wasm_bytes, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_session_with_incorrect_wasm_no_memory_section() {
    let wasm_bytes = make_module_without_memory_section();
    let execution_phase = ExecutionPhase::Session;
    let expected_error = "Memory section should exist";
    run_test_case(&wasm_bytes, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_payment_with_incorrect_wasm_start_section() {
    let wasm_bytes = make_module_with_start_section();
    let execution_phase = ExecutionPhase::Payment;
    let expected_error = "Unsupported WASM start";
    run_test_case(&wasm_bytes, expected_error, execution_phase)
}

#[ignore]
#[test]
fn should_charge_session_with_incorrect_wasm_start_section() {
    let wasm_bytes = make_module_with_start_section();
    let execution_phase = ExecutionPhase::Session;
    let expected_error = "Unsupported WASM start";
    run_test_case(&wasm_bytes, expected_error, execution_phase)
}
