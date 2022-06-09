use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::engine_state::MAX_PAYMENT;
use casper_types::{runtime_args, Gas, RuntimeArgs, U512};
use num_traits::Zero;

use crate::{
    test::regression::test_utils::{make_gas_counter_overflow, make_module_without_memory_section},
    wasm_utils,
};

const ARG_AMOUNT: &str = "amount";

struct TestCase<'a> {
    pub id: String,
    pub input_wasm_bytes: &'a [u8],
    pub expected_error_payment: String,
    pub expected_error_session: String,
}

impl<'a> TestCase<'a> {
    fn new(
        id: String,
        input_wasm_bytes: &'a [u8],
        expected_error_payment: String,
        expected_error_session: String,
    ) -> Self {
        Self {
            id,
            input_wasm_bytes,
            expected_error_payment,
            expected_error_session,
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum DeployBytes {
    Payment,
    Session,
}

fn run_test_case(
    TestCase {
        id,
        input_wasm_bytes,
        expected_error_payment,
        expected_error_session,
    }: &TestCase,
    deploy_bytes: DeployBytes,
) {
    dbg!(&id);
    dbg!(&deploy_bytes);

    let payment_amount = *DEFAULT_PAYMENT;

    let expected_transaction_fee = *MAX_PAYMENT;

    let (do_minimum_request_builder, expected_error_message) = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash = [42; 32];

        let (deploy_item_builder, expected_error_message) = match deploy_bytes {
            DeployBytes::Payment => (
                DeployItemBuilder::new()
                    .with_payment_bytes(
                        input_wasm_bytes.to_vec(),
                        runtime_args! {ARG_AMOUNT => U512::from(payment_amount),},
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), session_args),
                expected_error_payment,
            ),
            DeployBytes::Session => (
                DeployItemBuilder::new()
                    .with_session_bytes(input_wasm_bytes.to_vec(), session_args)
                    .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => payment_amount,}),
                expected_error_session,
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

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();

    let proposer_balance_before = builder.get_proposer_purse_balance();

    let account_balance_before = builder.get_purse_balance(account.main_purse());

    builder.exec(do_minimum_request).expect_failure().commit();

    let actual_error = builder.get_error().expect("should have error").to_string();
    dbg!(&actual_error);
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

#[ignore]
#[test]
fn should_charge_session_with_incorrect_wasm_files() {
    // This test focuses on testing whether we charge for
    // WASM files that are malformed (unparseable).

    // If we're provided with such file, we should charge.
    // The expection here is the "empty wasm" send as
    // a payment, because in such case we use the "default payment"
    // instead.

    // For increased security, we also test some other cases in this test
    // like gas overflow (which is a runtime error).

    // TODO:
    // Introduce proper layers in wasm execution, for example:
    // Layer 1. Parse (may throw errors related to malformed wasm bytes)
    // Layer 2. Validation (may throw errors for parseable, but unsupported
    //             wasm files, for example, without memory session)
    // Layer 3. Execution (may throw errors strictly related to execution
    //             for example, stack overflow)

    const WASM_BYTES_INVALID_MAGIC_NUMBER: &[u8] = &[1, 2, 3, 4, 5]; // Correct WASM magic bytes are: 0x00 0x61 0x73 0x6d ("\0asm")

    const WASM_BYTES_EMPTY: &[u8] = &[];

    // These are the first few bytes of the valid WASM module.
    const WASM_BYTES_INVALID_BUT_WITH_CORRECT_MAGIC_NUMBER: &[u8] = &[
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

    let wasm_bytes_gas_overflow = make_gas_counter_overflow();

    let wasm_bytes_no_memory_section = make_module_without_memory_section();

    // Other potential test cases:
    // 1. Wasm that doesn't have "call" function    - tested in `regression_20210924`
    // 2. Wasm with unsupported "start" section     - tested in `ee_890` (but without asserting the charge)

    let test_cases: &[TestCase; 4] = {
        &[
            TestCase::new(
                "WASM_BYTES_INVALID_MAGIC_NUMBER".to_string(),
                WASM_BYTES_INVALID_MAGIC_NUMBER,
                "Wasm preprocessing error: Deserialization error: Invalid magic number at start of file".to_string(),
                "Wasm preprocessing error: Deserialization error: Invalid magic number at start of file".to_string(),
            ),
            // TestCase::new(
            //     "WASM_BYTES_EMPTY".to_string(),
            //     WASM_BYTES_EMPTY,
            //     "Insufficient payment".to_string(),
            //     "Wasm preprocessing error: Deserialization error: I/O Error: UnexpectedEof"
            //         .to_string(),
            // ),
            TestCase::new(
                "WASM_BYTES_INVALID_BUT_WITH_CORRECT_MAGIC_NUMBER".to_string(),
                WASM_BYTES_INVALID_BUT_WITH_CORRECT_MAGIC_NUMBER,
                "I/O Error: UnexpectedEof".to_string(),
                "I/O Error: UnexpectedEof".to_string()
            ),
            TestCase::new(
                "wasm_bytes_gas_overflow".to_string(),
                wasm_bytes_gas_overflow.as_slice(),
                "Wasm preprocessing error: Encountered operation forbidden by gas rules. Consult instruction -> metering config map".to_string(),
                "Wasm preprocessing error: Encountered operation forbidden by gas rules. Consult instruction -> metering config map".to_string(),
            ),
            TestCase::new(
                "wasm_bytes_no_memory_section".to_string(),
                wasm_bytes_no_memory_section.as_slice(),
                "Memory section should exist".to_string(),
                "Memory section should exist".to_string(),
            ),
        ]
    };

    [DeployBytes::Payment, DeployBytes::Session]
        .iter()
        .for_each(|deploy_bytes| {
            test_cases
                .iter()
                .for_each(|test_case| run_test_case(test_case, *deploy_bytes))
        })
}
