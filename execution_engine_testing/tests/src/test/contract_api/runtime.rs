use std::collections::HashSet;

use rand::Rng;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::ADDRESS_LENGTH;
use casper_types::{crypto, runtime_args, RuntimeArgs, BLAKE2B_DIGEST_LENGTH};

const BLAKE2B_WASM: &str = "blake2b.wasm";
const ARG_BYTES: &str = "bytes";
const HASH_RESULT: &str = "hash_result";

const NEXT_ADDRESS_WASM: &str = "next_address.wasm";
const NEXT_ADDRESS_RESULT: &str = "next_address_result";

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
fn should_return_distinct_next_addresses() {
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
