use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution::Error as ExecError};
use casper_types::RuntimeArgs;

#[ignore]
#[test]
fn runtime_stack_overflow_should_cause_unreachable_error() {
    // Create an unconstrained recursive call
    let wat = r#"(module
        (func $call (call $call))
        (export "call" (func $call))
        (memory $memory 1)
      )"#;

    let module_bytes = wabt::wat2wasm(wat).unwrap();

    let do_stack_overflow_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());
    builder
        .exec(do_stack_overflow_request)
        .expect_failure()
        .commit();

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
