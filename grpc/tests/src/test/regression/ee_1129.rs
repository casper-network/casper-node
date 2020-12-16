use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS,
        DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casper_execution_engine::{
    core::{
        engine_state::{genesis::GenesisAccount, Error},
        execution,
    },
    shared::motes::Motes,
};
use casper_types::{
    account::AccountHash,
    auction::{self, DelegationRate},
    runtime_args, PublicKey, RuntimeArgs, U512,
};

const ENTRY_POINT_NAME: &str = "create_purse";
const CONTRACT_KEY: &str = "contract";
const CONTRACT_EE_1129_REGRESSION: &str = "ee_1129_regression.wasm";

const ARG_AMOUNT: &str = "amount";

const VALIDATOR_1: PublicKey = PublicKey::Ed25519([3; 32]);
static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| VALIDATOR_1.into());
const VALIDATOR_1_STAKE: u64 = 250_000;
static UNDERFUNDED_PAYMENT_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(10_000));
static CALL_STORED_CONTRACT_OVERHEAD: Lazy<U512> = Lazy::new(|| U512::from(6_000_000));

#[ignore]
#[test]
fn should_run_ee_1129_underfunded_delegate_call() {
    let payment_amount = *UNDERFUNDED_PAYMENT_AMOUNT;

    let accounts = {
        let validator_1 = GenesisAccount::new(
            VALIDATOR_1,
            *VALIDATOR_1_ADDR,
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Motes::new(VALIDATOR_1_STAKE.into()),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&run_genesis_request);

    let auction = builder.get_auction_contract_hash();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let bid_amount = U512::one();

    let deploy_hash = [42; 32];

    let source_purse = account.main_purse();

    let args = runtime_args! {
        auction::ARG_DELEGATOR => *DEFAULT_ACCOUNT_PUBLIC_KEY,
        auction::ARG_VALIDATOR => VALIDATOR_1,
        auction::ARG_SOURCE_PURSE => source_purse,
        auction::ARG_AMOUNT => bid_amount,
    };

    let deploy = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_stored_session_hash(auction, auction::METHOD_DELEGATE, args)
        .with_empty_payment_bytes(runtime_args! {
            ARG_AMOUNT => payment_amount, // underfunded deploy
        })
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash(deploy_hash)
        .build();

    let exec_request = ExecuteRequestBuilder::new().push_deploy(deploy).build();

    builder.exec(exec_request).commit().expect_success();
}

#[ignore]
#[test]
fn should_run_ee_1129_underfunded_add_bid_call() {
    let payment_amount = *UNDERFUNDED_PAYMENT_AMOUNT;

    let accounts = {
        let validator_1 = GenesisAccount::new(
            VALIDATOR_1,
            *VALIDATOR_1_ADDR,
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Motes::new(U512::zero()), //VALIDATOR_1_STAKE.into()),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp
    };
    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&run_genesis_request);

    let auction = builder.get_auction_contract_hash();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let deploy_hash = [42; 32];

    let unbond_purse = account.main_purse();
    let amount = U512::one();

    let delegation_rate: DelegationRate = 10;

    let args = runtime_args! {
            auction::ARG_PUBLIC_KEY => VALIDATOR_1,
            auction::ARG_SOURCE_PURSE => unbond_purse,
            auction::ARG_AMOUNT => amount,
            auction::ARG_DELEGATION_RATE => delegation_rate,
    };

    let deploy = DeployItemBuilder::new()
        .with_address(*VALIDATOR_1_ADDR)
        .with_stored_session_hash(auction, auction::METHOD_ADD_BID, args)
        .with_empty_payment_bytes(runtime_args! {
            ARG_AMOUNT => payment_amount,
        })
        .with_authorization_keys(&[*VALIDATOR_1_ADDR])
        .with_deploy_hash(deploy_hash)
        .build();

    let exec_request = ExecuteRequestBuilder::new().push_deploy(deploy).build();

    builder.exec(exec_request).commit().expect_success();
}

#[ignore]
#[test]
fn should_run_ee_1129_underfunded_mint_contract_call() {
    let payment_amount = *CALL_STORED_CONTRACT_OVERHEAD;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_1129_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_named_key(CONTRACT_KEY, ENTRY_POINT_NAME, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_amount,
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(install_exec_request).expect_success().commit();

    builder.exec(exec_request).commit();

    let error = builder
        .get_exec_responses()
        .last()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::GasLimit)),
        "Unexpected error {:?}",
        error
    );
}
