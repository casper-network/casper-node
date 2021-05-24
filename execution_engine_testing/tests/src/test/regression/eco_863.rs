use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS},
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{
    core::{
        engine_state::{self, genesis::GenesisAccount},
        execution,
    },
    shared::motes::Motes,
};
use casper_types::{
    account::AccountHash, runtime_args, ApiError, Key, PublicKey, RuntimeArgs, SecretKey, U512,
};

const CONTRACT_FAUCET_STORED: &str = "faucet_stored.wasm";
const CONTRACT_FAUCET_ENTRYPOINT: &str = "call_faucet";
const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

const FAUCET_REQUEST_AMOUNT: u64 = 333_333_333;

static FAUCET: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([1; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ALICE: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([2; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

static FAUCET_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*FAUCET));
static ALICE_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ALICE));

#[ignore]
#[test]
fn faucet_should_create_account() {
    let accounts = {
        let faucet_account = GenesisAccount::account(
            FAUCET.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            None,
        );
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(faucet_account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&run_genesis_request);

    let store_faucet_request = ExecuteRequestBuilder::standard(
        *FAUCET_ADDR,
        CONTRACT_FAUCET_STORED,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(store_faucet_request).expect_success().commit();

    let faucet_account = {
        let tmp = builder
            .query(None, Key::Account(*FAUCET_ADDR), &[])
            .unwrap();
        tmp.as_account().cloned().unwrap()
    };

    let faucet_hash = {
        let faucet_key = faucet_account.named_keys().get("faucet").cloned().unwrap();
        faucet_key.into_hash().unwrap()
    };

    let faucet_request_amount = U512::from(FAUCET_REQUEST_AMOUNT);

    let faucet_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        faucet_hash.into(),
        CONTRACT_FAUCET_ENTRYPOINT,
        runtime_args! {
            ARG_TARGET => *ALICE_ADDR,
            ARG_AMOUNT => faucet_request_amount,
        },
    )
    .build();

    builder.exec(faucet_request).commit().expect_success();

    let alice_account = {
        let tmp = builder.query(None, Key::Account(*ALICE_ADDR), &[]).unwrap();
        tmp.as_account().cloned().unwrap()
    };

    let balance = builder.get_purse_balance(alice_account.main_purse());

    assert_eq!(balance, faucet_request_amount);

    let faucet_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        faucet_hash.into(),
        CONTRACT_FAUCET_ENTRYPOINT,
        runtime_args! {
            ARG_TARGET => *ALICE_ADDR,
            ARG_AMOUNT => faucet_request_amount,
        },
    )
    .build();

    builder.exec(faucet_request).commit();

    let error = {
        let response = builder
            .get_exec_results()
            .last()
            .expect("should have last exec result");
        let exec_response = response.last().expect("should have response");
        exec_response.as_error().expect("should have error")
    };
    assert_matches!(
        error,
        engine_state::Error::Exec(execution::Error::Revert(ApiError::User(1)))
    );
}

#[ignore]
#[test]
fn faucet_should_transfer_to_existing_account() {
    let accounts = {
        let faucet_account = GenesisAccount::account(
            FAUCET.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            None,
        );
        let alice_account = GenesisAccount::account(
            ALICE.clone(),
            Motes::new(MINIMUM_ACCOUNT_CREATION_BALANCE.into()),
            None,
        );
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(faucet_account);
        tmp.push(alice_account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&run_genesis_request);

    let store_faucet_request = ExecuteRequestBuilder::standard(
        *FAUCET_ADDR,
        CONTRACT_FAUCET_STORED,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(store_faucet_request).expect_success().commit();

    let faucet_account = {
        let tmp = builder
            .query(None, Key::Account(*FAUCET_ADDR), &[])
            .unwrap();
        tmp.as_account().cloned().unwrap()
    };

    let faucet_hash = {
        let faucet_key = faucet_account.named_keys().get("faucet").cloned().unwrap();
        faucet_key.into_hash().unwrap()
    };

    let faucet_request_amount = U512::from(FAUCET_REQUEST_AMOUNT);

    let faucet_request = ExecuteRequestBuilder::contract_call_by_hash(
        *FAUCET_ADDR,
        faucet_hash.into(),
        CONTRACT_FAUCET_ENTRYPOINT,
        runtime_args! {
            ARG_TARGET => *ALICE_ADDR,
            ARG_AMOUNT => faucet_request_amount,
        },
    )
    .build();

    builder.exec(faucet_request).commit().expect_success();

    let alice_account = {
        let tmp = builder.query(None, Key::Account(*ALICE_ADDR), &[]).unwrap();
        tmp.as_account().cloned().unwrap()
    };

    let balance = builder.get_purse_balance(alice_account.main_purse());

    assert_eq!(
        balance,
        faucet_request_amount + MINIMUM_ACCOUNT_CREATION_BALANCE
    );

    let faucet_request = ExecuteRequestBuilder::contract_call_by_hash(
        *FAUCET_ADDR,
        faucet_hash.into(),
        CONTRACT_FAUCET_ENTRYPOINT,
        runtime_args! {
            ARG_TARGET => *ALICE_ADDR,
            ARG_AMOUNT => faucet_request_amount,
        },
    )
    .build();

    builder.exec(faucet_request).commit();

    let error = {
        let response = builder
            .get_exec_results()
            .last()
            .expect("should have last exec result");
        let exec_response = response.last().expect("should have response");
        exec_response.as_error().expect("should have error")
    };
    assert_matches!(
        error,
        engine_state::Error::Exec(execution::Error::Revert(ApiError::User(1)))
    );
}
