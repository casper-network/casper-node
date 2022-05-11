use std::collections::HashSet;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, RuntimeArgs, KEY_HASH_LENGTH};

const NEXT_ADDRESS_WASM: &str = "next_address.wasm";
const NEXT_ADDRESS_RESULT: &str = "next_address_result";

fn get_next_address(builder: &InMemoryWasmTestBuilder) -> [u8; KEY_HASH_LENGTH] {
    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let uref = account
        .named_keys()
        .get(NEXT_ADDRESS_RESULT)
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
fn should_return_address() {
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

            get_next_address(&builder)
        })
        .collect();

    // Assert that each address is unique.
    assert_eq!(all_addresses.len(), RUNS)
}
