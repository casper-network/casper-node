use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state::Error, execution::Error as ExecError};
use casper_types::{runtime_args, RuntimeArgs};

#[ignore]
#[test]
fn panic_in_contract_should_yield_trap_unreachable() {
    const CONTRACT_NAME: &str = "do_panic.wasm";

    let do_panic_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, CONTRACT_NAME, runtime_args! {})
            .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);
    builder.exec(do_panic_request).expect_failure().commit();

    // TODO: In order to assert if proper message has been put on `stderr` we might consider
    // extending EE to be able to write to arbitrary stream. It would default to `Stdout` so the
    // users can see the message and we can set it to a pipe or file in the test, so we can analyze
    // the output.

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(&error, Error::Exec(ExecError::Interpreter(s)) if s.contains("Unreachable")),
        "{:?}",
        error
    );
}
