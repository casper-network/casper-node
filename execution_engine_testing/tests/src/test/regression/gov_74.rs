use once_cell::sync::Lazy;

use casper_engine_test_support::{
    instrumented, ExecuteRequestBuilder, InMemoryWasmTestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PROTOCOL_VERSION, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::{
        engine_state::{EngineConfigBuilder, Error},
        execution::Error as ExecError,
    },
    shared::{
        wasm_config::WasmConfig,
        wasm_prep::{self, WasmValidationError, DEFAULT_MAX_PARAMETER_COUNT},
    },
};
use casper_types::{EraId, ProtocolVersion, RuntimeArgs};

use crate::wasm_utils;

const ARITY_INTERPRETER_LIMIT: usize = DEFAULT_MAX_PARAMETER_COUNT as usize;
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);
const I32_WAT_TYPE: &str = "i64";
const NEW_WASM_STACK_HEIGHT: u32 = 16;

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

fn initialize_builder() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);
    builder
}

#[ignore]
#[test]
fn should_pass_max_parameter_count() {
    let mut builder = initialize_builder();

    // This runs out of the interpreter stack limit
    let module_bytes = wasm_utils::make_n_arg_call_bytes(ARITY_INTERPRETER_LIMIT, I32_WAT_TYPE)
        .expect("should make wasm bytes");

    let exec = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec_instrumented(exec, instrumented!())
        .expect_success()
        .commit();

    let module_bytes = wasm_utils::make_n_arg_call_bytes(ARITY_INTERPRETER_LIMIT + 1, I32_WAT_TYPE)
        .expect("should make wasm bytes");

    let exec = ExecuteRequestBuilder::module_bytes(
        *DEFAULT_ACCOUNT_ADDR,
        module_bytes,
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec_instrumented(exec, instrumented!())
        .expect_failure()
        .commit();
    let error = builder.get_error().expect("should have error");

    assert!(
        matches!(
            error,
            Error::WasmPreprocessing(wasm_prep::PreprocessingError::WasmValidation(
                WasmValidationError::TooManyParameters {
                    max: 256,
                    actual: 257
                }
            ))
        ),
        "{:?}",
        error
    );
}

#[ignore]
#[test]
fn should_observe_stack_height_limit() {
    let mut builder = initialize_builder();

    assert!(WasmConfig::default().max_stack_height > NEW_WASM_STACK_HEIGHT);

    // This runs out of the interpreter stack limit
    let exec_request_1 = {
        let module_bytes =
            wasm_utils::make_n_arg_call_bytes(NEW_WASM_STACK_HEIGHT as usize, I32_WAT_TYPE)
                .expect("should make wasm bytes");

        ExecuteRequestBuilder::module_bytes(
            *DEFAULT_ACCOUNT_ADDR,
            module_bytes,
            RuntimeArgs::default(),
        )
        .build()
    };

    builder
        .exec_instrumented(exec_request_1, instrumented!())
        .expect_success()
        .commit();

    {
        // We need to perform an upgrade to be able to observe new max wasm stack height.
        let new_engine_config = EngineConfigBuilder::default()
            .with_wasm_max_stack_height(NEW_WASM_STACK_HEIGHT)
            .build();

        let mut upgrade_request = UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build();

        builder
            .upgrade_with_upgrade_request_and_config(Some(new_engine_config), &mut upgrade_request);
    }

    // This runs out of the interpreter stack limit.
    // An amount of args equal to the new limit fails because there's overhead of `fn call` that
    // adds 1 to the height.
    let exec_request_2 = {
        let module_bytes =
            wasm_utils::make_n_arg_call_bytes(NEW_WASM_STACK_HEIGHT as usize, I32_WAT_TYPE)
                .expect("should make wasm bytes");

        ExecuteRequestBuilder::module_bytes(
            *DEFAULT_ACCOUNT_ADDR,
            module_bytes,
            RuntimeArgs::default(),
        )
        .with_protocol_version(*NEW_PROTOCOL_VERSION)
        .build()
    };

    builder
        .exec_instrumented(exec_request_2, instrumented!())
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(&error, Error::Exec(ExecError::Interpreter(s)) if s.contains("Unreachable")),
        "{:?}",
        error
    );

    // But new limit minus one runs fine
    let exec_request_3 = {
        let module_bytes =
            wasm_utils::make_n_arg_call_bytes(NEW_WASM_STACK_HEIGHT as usize - 1, I32_WAT_TYPE)
                .expect("should make wasm bytes");

        ExecuteRequestBuilder::module_bytes(
            *DEFAULT_ACCOUNT_ADDR,
            module_bytes,
            RuntimeArgs::default(),
        )
        .with_protocol_version(*NEW_PROTOCOL_VERSION)
        .build()
    };

    builder
        .exec_instrumented(exec_request_3, instrumented!())
        .expect_success()
        .commit();
}
