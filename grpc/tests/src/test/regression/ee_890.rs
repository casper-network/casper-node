use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, ARG_AMOUNT,
        DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{runtime_args, RuntimeArgs};

const DO_NOTHING_WASM: &str = "do_nothing.wasm";

// NOTE: Apparently rustc does not emit "start" when targeting wasm32
// Ref: https://github.com/rustwasm/team/issues/108
const CONTRACT_WAT_WITH_START: &str = r#"
(module
    (memory (;0;) 1)
    (export "memory" (memory 0))
    (type (;0;) (func))
    (func (;0;) (type 0)
      nop)
    (start 0))
"#;

#[ignore]
#[test]
fn should_run_ee_890_gracefully_reject_start_node_in_session() {
    let wasm_binary = wabt::wat2wasm(CONTRACT_WAT_WITH_START).expect("should parse");

    let deploy_1 = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_bytes(wasm_binary, RuntimeArgs::new())
        .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([123; 32])
        .build();

    let exec_request_1 = ExecuteRequestBuilder::new().push_deploy(deploy_1).build();

    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .commit()
        .finish();
    let message = result.builder().exec_error_message(0).expect("should fail");
    assert!(
        message.contains("UnsupportedWasmStart"),
        "Error message {:?} does not contain expected pattern",
        message
    );
}

#[ignore]
#[test]
fn should_run_ee_890_gracefully_reject_start_node_in_payment() {
    let wasm_binary = wabt::wat2wasm(CONTRACT_WAT_WITH_START).expect("should parse");

    let deploy_1 = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::new())
        .with_payment_bytes(wasm_binary, RuntimeArgs::new())
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([123; 32])
        .build();

    let exec_request_1 = ExecuteRequestBuilder::new().push_deploy(deploy_1).build();

    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .commit()
        .finish();
    let message = result.builder().exec_error_message(0).expect("should fail");
    assert!(
        message.contains("UnsupportedWasmStart"),
        "Error message {:?} does not contain expected pattern",
        message
    );
}
