use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_PATH,
};
use casper_execution_engine::core::{
    engine_state::{self, genesis::GenesisAccount},
    execution,
};
use casper_types::{
    account::AccountHash, runtime_args, system::mint, ApiError, Key, Motes, PublicKey, RuntimeArgs,
    SecretKey, U512,
};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";
const CONTRACT_FAUCET_STORED: &str = "faucet_stored.wasm";
const CONTRACT_FAUCET_ENTRYPOINT: &str = "call_faucet";
const FAUCET_REQUEST_AMOUNT: u64 = 333_333_333;
const NEW_ACCOUNT_ADDR: AccountHash = AccountHash::new([99u8; 32]);

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

fn get_builder() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    {
        // first, store contract
        let store_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_FAUCET_STORED,
            runtime_args! {},
        )
        .build();

        builder.run_genesis_with_default_genesis_accounts();
        builder.exec(store_request).commit();
    }
    builder
}

#[ignore]
#[test]
fn should_fail_to_get_funds_from_faucet_stored() {
    let mut builder = get_builder();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash = default_account
        .named_keys()
        .get("faucet")
        .expect("contract_hash should exist")
        .into_hash()
        .expect("should be a hash");

    let amount = U512::from(1000);

    // call stored faucet
    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash.into(),
        CONTRACT_FAUCET_ENTRYPOINT,
        runtime_args! { ARG_TARGET => NEW_ACCOUNT_ADDR, ARG_AMOUNT => amount },
    )
    .build();

    builder.exec(exec_request).commit();

    match builder.get_error() {
        Some(engine_state::Error::Exec(execution::Error::Revert(api_error)))
            if api_error == ApiError::from(mint::Error::InvalidContext) => {}
        _ => panic!("should be an error"),
    }
}

#[ignore]
#[test]
fn should_fail_to_create_account() {
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

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder.run_genesis_with_custom_genesis_accounts(accounts);

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

    builder.exec(faucet_request).commit();

    match builder.get_error() {
        Some(engine_state::Error::Exec(execution::Error::Revert(api_error)))
            if api_error == ApiError::from(mint::Error::InvalidContext) => {}
        _ => panic!("should be an error"),
    }
}

#[ignore]
#[test]
fn should_fail_transfer_to_existing_account() {
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

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder.run_genesis_with_custom_genesis_accounts(accounts);

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

    builder.exec(faucet_request).commit();

    match builder.get_error() {
        Some(engine_state::Error::Exec(execution::Error::Revert(api_error)))
            if api_error == ApiError::from(mint::Error::InvalidContext) => {}
        _ => panic!("should be an error"),
    }
}
