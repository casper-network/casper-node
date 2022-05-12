use std::collections::HashSet;

use rand::Rng;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::ADDRESS_LENGTH;
use casper_types::{crypto, runtime_args, RuntimeArgs, BLAKE2B_DIGEST_LENGTH};

const ARG_BYTES: &str = "bytes";
const ARG_AMOUNT: &str = "amount";

const BLAKE2B_WASM: &str = "blake2b.wasm";
const HASH_RESULT: &str = "hash_result";

const NEXT_ADDRESS_WASM: &str = "next_address.wasm";
const NEXT_ADDRESS_RESULT: &str = "next_address_result";

const NEXT_ADDRESS_PAYMENT_WASM: &str = "next_address_payment.wasm";
const NEXT_ADDRESS_PAYMENT_RESULT: &str = "next_address_payment_result";

fn get_value<const COUNT: usize>(builder: &InMemoryWasmTestBuilder, result: &str) -> [u8; COUNT] {
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let uref = account.named_keys().get(result).expect("should have value");

    builder
        .query(None, *uref, &[])
        .expect("should query")
        .as_cl_value()
        .cloned()
        .expect("should be CLValue")
        .into_t()
        .expect("should convert")
}

#[ignore]
#[test]
fn should_return_different_first_addresses_on_different_phases() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let execute_request = {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();
        let address = *DEFAULT_ACCOUNT_ADDR;
        let deploy = DeployItemBuilder::new()
            .with_address(address)
            .with_session_code(NEXT_ADDRESS_WASM, runtime_args! {})
            .with_payment_code(
                NEXT_ADDRESS_PAYMENT_WASM,
                runtime_args! {
                    ARG_AMOUNT => *DEFAULT_PAYMENT
                },
            )
            .with_authorization_keys(&[address])
            .with_deploy_hash(deploy_hash)
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(execute_request).commit().expect_success();

    let session_generated_address = get_value::<ADDRESS_LENGTH>(&builder, NEXT_ADDRESS_RESULT);
    let payment_generated_address =
        get_value::<ADDRESS_LENGTH>(&builder, NEXT_ADDRESS_PAYMENT_RESULT);

    assert_ne!(session_generated_address, payment_generated_address)
}

#[ignore]
#[test]
fn should_return_different_next_addresses_on_each_call() {
    const RUNS: usize = 10;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let all_addresses: HashSet<_> = (0..RUNS)
        .map(|_| {
            let exec_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                NEXT_ADDRESS_WASM,
                runtime_args! {},
            )
            .build();

            builder.exec(exec_request).commit().expect_success();

            get_value::<ADDRESS_LENGTH>(&builder, NEXT_ADDRESS_RESULT)
        })
        .collect();

    // Assert that each address is unique.
    assert_eq!(all_addresses.len(), RUNS)
}

#[ignore]
#[test]
fn should_hash() {
    const INPUT_LENGTH: usize = 32;
    const RUNS: usize = 100;

    let mut rng = rand::thread_rng();
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    for _ in 0..RUNS {
        let input: [u8; INPUT_LENGTH] = rng.gen();

        let exec_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            BLAKE2B_WASM,
            runtime_args! {
                ARG_BYTES => input
            },
        )
        .build();

        builder.exec(exec_request).commit().expect_success();

        let digest = get_value::<BLAKE2B_DIGEST_LENGTH>(&builder, HASH_RESULT);
        let expected_digest = crypto::blake2b(&input);
        assert_eq!(digest, expected_digest);
    }
}
