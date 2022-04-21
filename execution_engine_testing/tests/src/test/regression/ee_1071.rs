use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::RuntimeArgs;

const CONTRACT_EE_1071_REGRESSION: &str = "ee_1071_regression.wasm";
const CONTRACT_HASH_NAME: &str = "contract";
const NEW_UREF_ENTRYPOINT: &str = "new_uref";

#[ignore]
#[test]
fn should_run_ee_1071_regression() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_1071_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash = (*account
        .named_keys()
        .get(CONTRACT_HASH_NAME)
        .expect("should have hash"))
    .into_hash()
    .expect("should be hash")
    .into();

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        NEW_UREF_ENTRYPOINT,
        RuntimeArgs::default(),
    )
    .build();

    let contract_before = builder
        .get_contract(contract_hash)
        .expect("should have account");

    builder.exec(exec_request_2).expect_success().commit();

    let contract_after = builder
        .get_contract(contract_hash)
        .expect("should have account");
    assert_ne!(
        contract_after, contract_before,
        "contract object should be modified"
    );
}
