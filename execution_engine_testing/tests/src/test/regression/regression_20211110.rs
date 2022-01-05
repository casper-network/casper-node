use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{
    engine_state::Error as CoreError, execution::Error as ExecError,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{mint, standard_payment},
    ContractHash, Key, RuntimeArgs, U512,
};

const RECURSE_ENTRYPOINT: &str = "recurse";
const ARG_TARGET: &str = "target";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";
const REGRESSION_20211110_CONTRACT: &str = "regression_20211110.wasm";

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([111; 32]);
const INSTALL_COST: u64 = 30_000_000_000;
const STARTING_BALANCE: u64 = 100_000_000_000;

#[ignore]
#[test]
fn regression_20211110() {
    let mut funds: u64 = STARTING_BALANCE;

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let transfer_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(funds),
            mint::ARG_ID => Option::<u64>::None
        },
    )
    .build();

    let install_request = {
        let session_args = runtime_args! {};
        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => U512::from(INSTALL_COST)
        };
        let deploy_item = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_empty_payment_bytes(payment_args)
            .with_session_code(REGRESSION_20211110_CONTRACT, session_args)
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder.exec(transfer_request).expect_success().commit();
    builder.exec(install_request).expect_success().commit();

    funds = funds.checked_sub(INSTALL_COST).unwrap();

    let contract_hash: ContractHash =
        match builder.get_expected_account(ACCOUNT_1_ADDR).named_keys()[CONTRACT_HASH_NAME] {
            Key::Hash(addr) => addr.into(),
            _ => panic!("Couldn't find regression contract."),
        };

    let recurse_request = {
        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => U512::from(funds),
        };
        let session_args = runtime_args! {
            ARG_TARGET => contract_hash
        };
        let deploy_item = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_empty_payment_bytes(payment_args)
            .with_stored_session_hash(contract_hash, RECURSE_ENTRYPOINT, session_args)
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([43; 32])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    builder.exec(recurse_request).expect_failure();

    let error = builder.get_error().expect("should have returned an error");
    assert!(matches!(
        error,
        CoreError::Exec(ExecError::RuntimeStackOverflow)
    ));
}
