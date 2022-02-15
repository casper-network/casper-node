use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_types::{RuntimeArgs, StoredValue};

const CONTRACT_NAME: &str = "do_nothing_stored.wasm";
const CONTRACT_PACKAGE_NAMED_KEY: &str = "do_nothing_package_hash";

#[ignore]
#[test]
fn should_query_contract_package() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let install_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, CONTRACT_NAME, RuntimeArgs::new())
            .build();

    builder.exec(install_request).expect_success().commit();

    let contract_package_hash = builder
        .get_expected_account(*DEFAULT_ACCOUNT_ADDR)
        .named_keys()
        .clone()
        .get(CONTRACT_PACKAGE_NAMED_KEY)
        .expect("failed to get contract package named key.")
        .to_owned();

    let contract_package = builder
        .query(None, contract_package_hash, &[])
        .expect("failed to find contract package");

    assert!(matches!(contract_package, StoredValue::ContractPackage(_)));
}
