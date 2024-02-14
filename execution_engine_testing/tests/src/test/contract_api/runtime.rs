use std::collections::HashSet;

use rand::Rng;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::runtime_context::RANDOM_BYTES_COUNT;
use casper_storage::address_generator::ADDRESS_LENGTH;
use casper_types::{crypto, runtime_args, BLAKE2B_DIGEST_LENGTH};

const ARG_BYTES: &str = "bytes";
const ARG_AMOUNT: &str = "amount";

const BLAKE2B_WASM: &str = "blake2b.wasm";
const HASH_RESULT: &str = "hash_result";

const RANDOM_BYTES_WASM: &str = "random_bytes.wasm";
const RANDOM_BYTES_RESULT: &str = "random_bytes_result";

const RANDOM_BYTES_PAYMENT_WASM: &str = "random_bytes_payment.wasm";
const RANDOM_BYTES_PAYMENT_RESULT: &str = "random_bytes_payment_result";

fn get_value<const COUNT: usize>(builder: &LmdbWasmTestBuilder, result: &str) -> [u8; COUNT] {
    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let uref = account.named_keys().get(result).expect("should have value");

    builder
        .query(None, *uref, &[])
        .expect("should query")
        .into_cl_value()
        .expect("should be CLValue")
        .into_t()
        .expect("should convert")
}

#[ignore]
#[test]
fn should_return_different_random_bytes_on_different_phases() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let execute_request = {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();
        let address = *DEFAULT_ACCOUNT_ADDR;
        let deploy = DeployItemBuilder::new()
            .with_address(address)
            .with_session_code(RANDOM_BYTES_WASM, runtime_args! {})
            .with_payment_code(
                RANDOM_BYTES_PAYMENT_WASM,
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

    let session_generated_bytes = get_value::<RANDOM_BYTES_COUNT>(&builder, RANDOM_BYTES_RESULT);
    let payment_generated_bytes =
        get_value::<ADDRESS_LENGTH>(&builder, RANDOM_BYTES_PAYMENT_RESULT);

    assert_ne!(session_generated_bytes, payment_generated_bytes)
}

#[ignore]
#[test]
fn should_return_different_random_bytes_on_each_call() {
    const RUNS: usize = 10;

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let all_addresses: HashSet<_> = (0..RUNS)
        .map(|_| {
            let exec_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                RANDOM_BYTES_WASM,
                runtime_args! {},
            )
            .build();

            builder.exec(exec_request).commit().expect_success();

            get_value::<RANDOM_BYTES_COUNT>(&builder, RANDOM_BYTES_RESULT)
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
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

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
        let expected_digest = crypto::blake2b(input);
        assert_eq!(digest, expected_digest);
    }
}
