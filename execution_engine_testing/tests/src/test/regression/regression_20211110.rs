use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, TransferRequestBuilder,
    LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error as CoreError, execution::ExecError};
use casper_types::{
    account::AccountHash, runtime_args, system::standard_payment, AddressableEntityHash, Key, U512,
};

const RECURSE_ENTRYPOINT: &str = "recurse";
const ARG_TARGET: &str = "target";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";
const REGRESSION_20211110_CONTRACT: &str = "regression_20211110.wasm";

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([111; 32]);
const INSTALL_COST: u64 = 40_000_000_000;
const STARTING_BALANCE: u64 = 100_000_000_000;

#[ignore]
#[test]
fn regression_20211110() {
    let mut funds: u64 = STARTING_BALANCE;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let transfer_request = TransferRequestBuilder::new(funds, ACCOUNT_1_ADDR).build();

    let session_args = runtime_args! {};
    let payment_args = runtime_args! {
        standard_payment::ARG_AMOUNT => U512::from(INSTALL_COST)
    };
    let deploy_item = DeployItemBuilder::new()
        .with_address(ACCOUNT_1_ADDR)
        .with_standard_payment(payment_args)
        .with_session_code(REGRESSION_20211110_CONTRACT, session_args)
        .with_authorization_keys(&[ACCOUNT_1_ADDR])
        .with_deploy_hash([42; 32])
        .build();

    let install_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    builder
        .transfer_and_commit(transfer_request)
        .expect_success();
    builder.exec(install_request).expect_success().commit();

    funds = funds.checked_sub(INSTALL_COST).unwrap();

    let contract_hash = match builder
        .get_entity_with_named_keys_by_account_hash(ACCOUNT_1_ADDR)
        .unwrap()
        .named_keys()
        .get(CONTRACT_HASH_NAME)
        .unwrap()
    {
        Key::AddressableEntity(entity_addr) => AddressableEntityHash::new(entity_addr.value()),
        _ => panic!("Couldn't find regression contract."),
    };

    let payment_args = runtime_args! {
        standard_payment::ARG_AMOUNT => U512::from(funds),
    };
    let session_args = runtime_args! {
        ARG_TARGET => contract_hash
    };
    let deploy_item = DeployItemBuilder::new()
        .with_address(ACCOUNT_1_ADDR)
        .with_standard_payment(payment_args)
        .with_stored_session_hash(contract_hash, RECURSE_ENTRYPOINT, session_args)
        .with_authorization_keys(&[ACCOUNT_1_ADDR])
        .with_deploy_hash([43; 32])
        .build();
    let recurse_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    builder.exec(recurse_request).expect_failure();

    let error = builder.get_error().expect("should have returned an error");
    assert!(matches!(
        error,
        CoreError::Exec(ExecError::RuntimeStackOverflow)
    ));
}
