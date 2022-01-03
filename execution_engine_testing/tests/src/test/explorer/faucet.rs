use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    account::AccountHash, crypto, runtime_args, system::mint, ApiError, PublicKey, RuntimeArgs,
    SecretKey, U512,
};

const FAUCET_INSTALLER_SESSION: &str = "faucet_stored.wasm";
const ENTRY_POINT_INIT: &str = "init";
const ARG_AMOUNT: &str = "amount";
const ARG_INSTALLER: &str = "installer";
const _ARG_TARGET: &str = "target";
const ARG_ID: &str = "id";
const _ARG_TIME_INCREMENT: &str = "time_increment";
const ARG_AVAILABLE_AMOUNT: &str = "available_amount";
const _TIME_INCREMENT: &str = "time_increment";
const _LAST_ISSUANCE: &str = "last_issuance";
const _FAUCET_PURSE: &str = "faucet_purse";
const _INSTALLER: &str = "installer";

#[ignore]
#[test]
fn should_install_faucet_contract() {
    let installer_secret_key =
        SecretKey::ed25519_from_bytes([1; 32]).expect("failed to create secret key");
    let installer_public_key = PublicKey::from(&installer_secret_key);
    let installer_account = AccountHash::from_public_key(&installer_public_key, crypto::blake2b);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST).commit();

    let fund_installer_account_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => installer_account,
            mint::ARG_AMOUNT => 300_000_000_000_000u64,
            mint::ARG_ID => <Option<u64>>::None
        },
    )
    .build();

    builder
        .exec(fund_installer_account_request)
        .expect_success()
        .commit();

    let installer_session_request = ExecuteRequestBuilder::standard(
        installer_account,
        FAUCET_INSTALLER_SESSION,
        runtime_args! {ARG_ID => 1337u64, ARG_AMOUNT => U512::from(500_000u64)},
    )
    .build();

    builder
        .exec(installer_session_request)
        .expect_success()
        .commit();

    // installer should be in contracts named keys.
    let keys = builder
        .get_expected_account(installer_account)
        .named_keys()
        .to_owned();

    println!("named keys: {:?}", keys);
}
