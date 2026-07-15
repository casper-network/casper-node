use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::runtime_args;

/// Builds a WAT session that imports `casper_get_named_arg` directly and requests the named
/// argument `arg` with a 64-byte destination size, even though the supplied `u32` only serializes
/// to 4 bytes. Without the fix the host slice `arg.inner_bytes()[..64]` panics.
fn raw_get_named_arg_with_oversized_buffer_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"
        (module
            (import "env" "casper_get_named_arg"
                (func $get_named_arg (param i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "arg")
            (func (export "call")
                i32.const 0
                i32.const 3
                i32.const 64
                i32.const 64
                call $get_named_arg
                drop
            )
        )
        "#,
    )
    .expect("wat must parse")
}

/// Regression for audit-confirmed-06: `casper_get_named_arg` must not panic when the caller-
/// supplied destination size exceeds the argument's serialized length.
#[ignore]
#[test]
fn should_not_panic_when_get_named_arg_buffer_is_larger_than_value() {
    let module_bytes = raw_get_named_arg_with_oversized_buffer_wasm();
    let exec_request = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        runtime_args! {
            "arg" => 1_u32,
        },
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .expect_success()
        .commit();
}
