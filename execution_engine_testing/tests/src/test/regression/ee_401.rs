use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
};
use casper_types::RuntimeArgs;

const CONTRACT_EE_401_REGRESSION: &str = "ee_401_regression.wasm";
const CONTRACT_EE_401_REGRESSION_CALL: &str = "ee_401_regression_call.wasm";

#[ignore]
#[test]
fn should_execute_contracts_which_provide_extra_urefs() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_401_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_401_REGRESSION_CALL,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder.run_genesis_with_default_genesis_accounts();
    builder.exec(exec_request_1).expect_success().commit();
    builder.exec(exec_request_2).expect_success().commit();
}
