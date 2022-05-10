use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::engine_state::ExecuteRequest;
use casper_types::{
    runtime_args, system::standard_payment::ARG_AMOUNT, ContractHash, ContractPackageHash,
    RuntimeArgs,
};

const GH_1688_REGRESSION: &str = "gh_1688_regression.wasm";

const METHOD_PUT_KEY: &str = "put_key";
const NEW_KEY_NAME: &str = "Hello";
const CONTRACT_PACKAGE_KEY: &str = "contract_package";
const CONTRACT_HASH_KEY: &str = "contract_hash";

fn setup() -> (InMemoryWasmTestBuilder, ContractPackageHash, ContractHash) {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let install_contract_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_1688_REGRESSION,
        runtime_args! {},
    )
    .build();

    builder
        .exec(install_contract_request_1)
        .expect_success()
        .commit();

    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();

    let contract_package_hash_key = account
        .named_keys()
        .get(CONTRACT_PACKAGE_KEY)
        .expect("should have package hash");

    let contract_hash_key = account
        .named_keys()
        .get(CONTRACT_HASH_KEY)
        .expect("should have hash");

    let contract_package_hash = contract_package_hash_key
        .into_hash()
        .map(ContractPackageHash::new)
        .expect("should be hash");

    let contract_hash = contract_hash_key
        .into_hash()
        .map(ContractHash::new)
        .expect("should be hash");

    (builder, contract_package_hash, contract_hash)
}

fn test(request_builder: impl FnOnce(ContractPackageHash, ContractHash) -> ExecuteRequest) {
    let (mut builder, contract_package_hash, contract_hash) = setup();

    let exec_request = request_builder(contract_package_hash, contract_hash);

    builder.exec(exec_request).expect_success().commit();

    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
    let contract = builder
        .get_contract(contract_hash)
        .expect("should have contract");

    assert!(
        contract.named_keys().contains_key(NEW_KEY_NAME),
        "expected {} in {:?}",
        NEW_KEY_NAME,
        contract.named_keys()
    );
    assert!(
        !account.named_keys().contains_key(NEW_KEY_NAME),
        "unexpected {} in {:?}",
        NEW_KEY_NAME,
        contract.named_keys()
    );
}

#[ignore]
#[test]
fn should_run_gh_1688_regression_stored_versioned_contract_by_hash() {
    test(|contract_package_hash, _contract_hash| {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_hash(
                contract_package_hash.value(),
                None,
                METHOD_PUT_KEY,
                RuntimeArgs::default(),
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    });
}

#[ignore]
#[test]
fn should_run_gh_1688_regression_stored_versioned_contract_by_name() {
    test(|_contract_package_hash, _contract_hash| {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                CONTRACT_PACKAGE_KEY,
                None,
                METHOD_PUT_KEY,
                RuntimeArgs::default(),
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    });
}

#[ignore]
#[test]
fn should_run_gh_1688_regression_stored_contract_by_hash() {
    test(|_contract_package_hash, contract_hash| {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_hash(contract_hash, METHOD_PUT_KEY, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    });
}

#[ignore]
#[test]
fn should_run_gh_1688_regression_stored_contract_by_name() {
    test(|_contract_package_hash, _contract_hash| {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_named_key(
                CONTRACT_HASH_KEY,
                METHOD_PUT_KEY,
                RuntimeArgs::default(),
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    });
}
