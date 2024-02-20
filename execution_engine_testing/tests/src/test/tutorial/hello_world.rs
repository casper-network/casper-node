use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, Key, StoredValue};

const HELLO_WORLD_CONTRACT: &str = "hello_world.wasm";
const KEY: &str = "special_value";
const ARG_MESSAGE: &str = "message";
const MESSAGE_VALUE: &str = "Hello, world!";

#[ignore]
#[test]
fn should_run_hello_world() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = {
        let session_args = runtime_args! {
            ARG_MESSAGE => MESSAGE_VALUE,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, HELLO_WORLD_CONTRACT, session_args)
            .build()
    };
    builder.exec(exec_request).expect_success().commit();

    let stored_message = builder
        .query(None, Key::from(*DEFAULT_ACCOUNT_ADDR), &[KEY.into()])
        .expect("should query");

    let message: String = if let StoredValue::CLValue(cl_value) = stored_message {
        cl_value.into_t().unwrap()
    } else {
        panic!("Stored message is not a clvalue: {:?}", stored_message);
    };
    assert_eq!(message, MESSAGE_VALUE);
}
