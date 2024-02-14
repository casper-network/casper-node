use casper_wasm::builder;
use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_types::{GenesisAccount, GenesisValidator};

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution, runtime::PreprocessingError};
use casper_types::{
    account::AccountHash,
    addressable_entity::DEFAULT_ENTRY_POINT_NAME,
    runtime_args,
    system::auction::{self, DelegationRate},
    Motes, PublicKey, RuntimeArgs, SecretKey, DEFAULT_ADD_BID_COST, DEFAULT_DELEGATE_COST, U512,
};

use crate::wasm_utils;

const ENTRY_POINT_NAME: &str = "create_purse";
const CONTRACT_KEY: &str = "contract";
const ACCESS_KEY: &str = "access";

const CONTRACT_EE_1129_REGRESSION: &str = "ee_1129_regression.wasm";
const ARG_AMOUNT: &str = "amount";

static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_1));
const VALIDATOR_1_STAKE: u64 = 250_000;
static UNDERFUNDED_DELEGATE_AMOUNT: Lazy<U512> =
    Lazy::new(|| U512::from(DEFAULT_DELEGATE_COST - 1));
static UNDERFUNDED_ADD_BID_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(DEFAULT_ADD_BID_COST - 1));
static CALL_STORED_CONTRACT_OVERHEAD: Lazy<U512> = Lazy::new(|| U512::from(10_001));

#[ignore]
#[test]
fn should_run_ee_1129_underfunded_delegate_call() {
    assert!(U512::from(DEFAULT_DELEGATE_COST) > *UNDERFUNDED_DELEGATE_AMOUNT);

    let accounts = {
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(run_genesis_request);

    let auction = builder.get_auction_contract_hash();

    let bid_amount = U512::one();

    let deploy_hash = [42; 32];

    let args = runtime_args! {
        auction::ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
        auction::ARG_AMOUNT => bid_amount,
    };

    let deploy = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_stored_session_hash(auction, auction::METHOD_DELEGATE, args)
        .with_empty_payment_bytes(runtime_args! {
            ARG_AMOUNT => *UNDERFUNDED_DELEGATE_AMOUNT, // underfunded deploy
        })
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash(deploy_hash)
        .build();

    let exec_request = ExecuteRequestBuilder::new().push_deploy(deploy).build();

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(
        matches!(error, Error::Exec(execution::Error::GasLimit)),
        "Unexpected error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_run_ee_1129_underfunded_add_bid_call() {
    assert!(U512::from(DEFAULT_ADD_BID_COST) > *UNDERFUNDED_ADD_BID_AMOUNT);

    let accounts = {
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            None,
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(run_genesis_request);

    let auction = builder.get_auction_contract_hash();

    let deploy_hash = [42; 32];

    let delegation_rate: DelegationRate = 10;

    let args = runtime_args! {
            auction::ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            auction::ARG_AMOUNT => *UNDERFUNDED_ADD_BID_AMOUNT,
            auction::ARG_DELEGATION_RATE => delegation_rate,
    };

    let deploy = DeployItemBuilder::new()
        .with_address(*VALIDATOR_1_ADDR)
        .with_stored_session_hash(auction, auction::METHOD_ADD_BID, args)
        .with_empty_payment_bytes(runtime_args! {
            ARG_AMOUNT => *UNDERFUNDED_DELEGATE_AMOUNT,
        })
        .with_authorization_keys(&[*VALIDATOR_1_ADDR])
        .with_deploy_hash(deploy_hash)
        .build();

    let exec_request = ExecuteRequestBuilder::new().push_deploy(deploy).build();

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(
        matches!(error, Error::Exec(execution::Error::GasLimit)),
        "Unexpected error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_run_ee_1129_underfunded_mint_contract_call() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

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
                ARG_AMOUNT => *CALL_STORED_CONTRACT_OVERHEAD,
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(install_exec_request).expect_success().commit();

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(
        matches!(error, Error::Exec(execution::Error::GasLimit)),
        "Unexpected error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_panic_when_calling_session_contract_by_uref() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_1129_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_named_key(ACCESS_KEY, ENTRY_POINT_NAME, RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *CALL_STORED_CONTRACT_OVERHEAD,
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(install_exec_request).expect_success().commit();

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(
        matches!(error, Error::InvalidKeyVariant),
        "Unexpected error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_panic_when_calling_payment_contract_by_uref() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_1129_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::new())
            .with_stored_payment_named_key(ACCESS_KEY, ENTRY_POINT_NAME, RuntimeArgs::new())
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(install_exec_request).expect_success().commit();

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(
        matches!(error, Error::InvalidKeyVariant),
        "Unexpected error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_panic_when_calling_contract_package_by_uref() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_1129_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                ACCESS_KEY,
                None,
                ENTRY_POINT_NAME,
                RuntimeArgs::default(),
            )
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *CALL_STORED_CONTRACT_OVERHEAD,
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(install_exec_request).expect_success().commit();

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(
        matches!(error, Error::InvalidKeyVariant),
        "Unexpected error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_panic_when_calling_payment_versioned_contract_by_uref() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let install_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_1129_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::new())
            .with_stored_versioned_payment_contract_by_name(
                ACCESS_KEY,
                None,
                ENTRY_POINT_NAME,
                RuntimeArgs::new(),
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(install_exec_request).expect_success().commit();

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");
    assert!(
        matches!(error, Error::InvalidKeyVariant),
        "Unexpected error {:?}",
        error
    );
}

fn do_nothing_without_memory() -> Vec<u8> {
    let module = builder::module()
        .function()
        // A signature with 0 params and no return type
        .signature()
        .build()
        .body()
        .build()
        .build()
        // Export above function
        .export()
        .field(DEFAULT_ENTRY_POINT_NAME)
        .build()
        .build();
    casper_wasm::serialize(module).expect("should serialize")
}

#[ignore]
#[test]
fn should_not_panic_when_calling_module_without_memory() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(do_nothing_without_memory(), RuntimeArgs::new())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT,
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(
        matches!(
            error,
            Error::WasmPreprocessing(PreprocessingError::MissingMemorySection)
        ),
        "Unexpected error {:?}",
        error
    );
}
