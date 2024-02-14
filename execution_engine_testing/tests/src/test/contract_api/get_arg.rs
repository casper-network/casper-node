use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, ApiError, RuntimeArgs, U512};

const CONTRACT_GET_ARG: &str = "get_arg.wasm";
const ARG0_VALUE: &str = "Hello, world!";
const ARG1_VALUE: u64 = 42;
const ARG_VALUE0: &str = "value0";
const ARG_VALUE1: &str = "value1";

/// Calls get_arg contract and returns Ok(()) in case no error, or String which is the error message
/// returned by the engine
fn call_get_arg(args: RuntimeArgs) -> Result<(), String> {
    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, CONTRACT_GET_ARG, args).build();
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit();

    if !builder.is_error() {
        return Ok(());
    }

    let response = builder
        .get_exec_result_owned(0)
        .expect("should have a response");

    let error_message = utils::get_error_message(response);

    Err(error_message)
}

#[ignore]
#[test]
fn should_use_passed_argument() {
    let args = runtime_args! {
        ARG_VALUE0 => ARG0_VALUE,
        ARG_VALUE1 => U512::from(ARG1_VALUE),
    };
    call_get_arg(args).expect("Should successfully call get_arg with 2 valid args");
}

#[ignore]
#[test]
fn should_revert_with_missing_arg() {
    assert!(call_get_arg(RuntimeArgs::default())
        .expect_err("should fail")
        .contains(&format!("{:?}", ApiError::MissingArgument),));
    assert!(
        call_get_arg(runtime_args! { ARG_VALUE0 => String::from(ARG0_VALUE) })
            .expect_err("should fail")
            .contains(&format!("{:?}", ApiError::MissingArgument))
    );
}

#[ignore]
#[test]
fn should_revert_with_invalid_argument() {
    let res1 =
        call_get_arg(runtime_args! {ARG_VALUE0 =>  U512::from(123)}).expect_err("should fail");
    assert!(
        res1.contains(&format!("{:?}", ApiError::InvalidArgument,)),
        "res1: {:?}",
        res1
    );

    let res2 = call_get_arg(runtime_args! {
        ARG_VALUE0 => String::from(ARG0_VALUE),
        ARG_VALUE1 => String::from("this is expected to be U512"),
    })
    .expect_err("should fail");

    assert!(
        res2.contains(&format!("{:?}", ApiError::InvalidArgument,)),
        "res2:{:?}",
        res2
    );
}
