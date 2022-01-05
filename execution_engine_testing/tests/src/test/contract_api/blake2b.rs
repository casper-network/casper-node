use rand::Rng;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_types::{crypto, runtime_args, RuntimeArgs, BLAKE2B_DIGEST_LENGTH};

const BLAKE2B_WASM: &str = "blake2b.wasm";
const ARG_BYTES: &str = "bytes";
const HASH_RESULT: &str = "hash_result";

fn get_digest(builder: &InMemoryWasmTestBuilder) -> [u8; BLAKE2B_DIGEST_LENGTH] {
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let uref = account
        .named_keys()
        .get(HASH_RESULT)
        .expect("should have value");

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
fn should_hash() {
    const INPUT_LENGTH: usize = 32;
    const RUNS: usize = 100;

    let mut rng = rand::thread_rng();
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

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

        let digest = get_digest(&builder);
        let expected_digest = crypto::blake2b(&input);
        assert_eq!(digest, expected_digest);
    }
}
