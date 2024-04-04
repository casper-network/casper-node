use casper_wasm::{self, builder};

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, ARG_AMOUNT,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT, LOCAL_GENESIS_REQUEST,
};
use casper_types::{addressable_entity::DEFAULT_ENTRY_POINT_NAME, runtime_args, RuntimeArgs};

const DO_NOTHING_WASM: &str = "do_nothing.wasm";

// NOTE: Apparently rustc does not emit "start" when targeting wasm32
// Ref: https://github.com/rustwasm/team/issues/108

/// Creates minimal session code that does nothing but has start node
fn make_do_nothing_with_start() -> Vec<u8> {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        // main() marks given function as a start() node
        .main()
        .body()
        .build()
        .build()
        // Export above function
        .export()
        .field(DEFAULT_ENTRY_POINT_NAME)
        .build()
        // Memory section is mandatory
        .memory()
        .build()
        .build();

    casper_wasm::serialize(module).expect("should serialize")
}

#[ignore]
#[test]
fn should_run_ee_890_gracefully_reject_start_node_in_session() {
    let wasm_binary = make_do_nothing_with_start();

    let deploy_item = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_bytes(wasm_binary, RuntimeArgs::new())
        .with_standard_payment(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([123; 32])
        .build();

    let exec_request_1 = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request_1)
        .commit();
    let message = builder.get_error_message().expect("should fail");
    assert!(
        message.contains("Unsupported Wasm start"),
        "Error message {:?} does not contain expected pattern",
        message
    );
}

#[ignore]
#[test]
fn should_run_ee_890_gracefully_reject_start_node_in_payment() {
    let wasm_binary = make_do_nothing_with_start();

    let deploy_item = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::new())
        .with_payment_bytes(wasm_binary, RuntimeArgs::new())
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([123; 32])
        .build();

    let exec_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit();
    let message = builder.get_error_message().expect("should fail");
    assert!(
        message.contains("Unsupported Wasm start"),
        "Error message {:?} does not contain expected pattern",
        message
    );
}
