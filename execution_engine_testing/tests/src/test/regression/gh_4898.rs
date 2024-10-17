use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};

use casper_types::runtime_args;

const ARG_DATA: &str = "data";
const GH_4898_REGRESSION_WASM: &str = "gh_4898_regression.wasm";

#[ignore]
#[test]
fn should_not_contain_f64_opcodes() {
    let module_bytes = utils::read_wasm_file(GH_4898_REGRESSION_WASM);
    let wat = wasmprinter::print_bytes(module_bytes).expect("WASM parse error");
    assert!(!wat.contains("f64."), "WASM contains f64 opcodes");

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_4898_REGRESSION_WASM,
        runtime_args! {
            ARG_DATA => "account-hash-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
        },
    )
    .build();

    builder.exec(exec_request).commit();
}
