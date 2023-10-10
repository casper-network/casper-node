use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, Key};

const HELLO_WORLD_CONTRACT: &str = "hello_world.wasm";
const KEY: &str = "special_value";
const ARG_MESSAGE: &str = "message";
const MESSAGE_VALUE: &str = "Hello, world!";

#[ignore]
#[test]
fn should_run_hello_world() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let exec_request = {
        let session_args = runtime_args! {
            ARG_MESSAGE => MESSAGE_VALUE,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, HELLO_WORLD_CONTRACT, session_args)
            .build()
    };
    builder.exec(exec_request).expect_success().commit();

    let cl_value = builder
        .query_uref_value(None, Key::from(*DEFAULT_ACCOUNT_ADDR), &[KEY.into()])
        .expect("should query");

    let message: String = cl_value.into_t().unwrap();
    assert_eq!(message, MESSAGE_VALUE);
}
