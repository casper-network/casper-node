use once_cell::sync::Lazy;
use rand::Rng;
use tempfile::TempDir;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::{DeployItem, MAX_PAYMENT_AMOUNT};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::mint::{ARG_AMOUNT, ARG_ID, ARG_TARGET},
    PublicKey, RuntimeArgs, SecretKey, U512,
};

static TRANSFER_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(MAX_PAYMENT_AMOUNT));

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([234u8; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_1_PUBLIC_KEY.to_account_hash());

static ACCOUNT_2_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([210u8; 32]).unwrap());
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_2_SECRET_KEY));
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_2_PUBLIC_KEY.to_account_hash());

const TRANSFER_COST: u64 = 100_000_000;
const ID_NONE: Option<u64> = None;

#[ignore]
#[test]
fn should_transfer_to_account_with_correct_balances() {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.path());

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let pre_state_hash = builder.get_post_state_hash();

    // Default account to account 1
    let mut exec_builder = ExecuteRequestBuilder::new();
    exec_builder = exec_builder.push_deploy(transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
           ARG_TARGET => *ACCOUNT_1_ADDR,
           ARG_AMOUNT => U512::one(),
           ARG_ID => ID_NONE,
        },
    ));
    builder
        .scratch_exec_and_commit(exec_builder.build())
        .expect_success();

    builder.write_scratch_to_db();
    builder.flush_environment();

    assert_ne!(
        pre_state_hash,
        builder.get_post_state_hash(),
        "post state hash didn't change..."
    );

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get default account");

    let account1 = builder
        .get_entity_by_account_hash(*ACCOUNT_1_ADDR)
        .expect("should get account 1");

    let default_account_balance = builder.get_purse_balance(default_account.main_purse());
    let default_expected_balance =
        U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) - (U512::one() + TRANSFER_COST);
    assert_eq!(
        default_account_balance, default_expected_balance,
        "default account balance should reflect the transfer",
    );

    let account_1_balance = builder.get_purse_balance(account1.main_purse());
    assert_eq!(
        account_1_balance,
        U512::one(),
        "account 1 balance should have been exactly one (1)"
    );
}

#[ignore]
#[test]
fn should_transfer_from_default_and_then_to_another_account() {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.path());

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let pre_state_hash = builder.get_post_state_hash();

    // Default account to account 1
    let mut exec_builder = ExecuteRequestBuilder::new();
    // We must first transfer the amount account 1 will transfer to account 2, along with the fee
    // account 1 will need to pay for that transfer.
    exec_builder = exec_builder.push_deploy(transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
           ARG_TARGET => *ACCOUNT_1_ADDR,
           ARG_AMOUNT => *TRANSFER_AMOUNT + TRANSFER_COST,
           ARG_ID => ID_NONE,
        },
    ));
    builder
        .scratch_exec_and_commit(exec_builder.build())
        .expect_success();

    let mut exec_builder = ExecuteRequestBuilder::new();
    exec_builder = exec_builder.push_deploy(transfer(
        *ACCOUNT_1_ADDR,
        runtime_args! {
            ARG_TARGET => *ACCOUNT_2_ADDR,
            ARG_AMOUNT => *TRANSFER_AMOUNT,
            ARG_ID => ID_NONE,
        },
    ));

    builder
        .scratch_exec_and_commit(exec_builder.build())
        .expect_success();

    // Double spend test for account 1
    let mut exec_builder = ExecuteRequestBuilder::new();
    exec_builder = exec_builder.push_deploy(transfer(
        *ACCOUNT_1_ADDR,
        runtime_args! {
            ARG_TARGET => *ACCOUNT_2_ADDR,
            ARG_AMOUNT => *TRANSFER_AMOUNT,
            ARG_ID => ID_NONE,
        },
    ));

    builder
        .scratch_exec_and_commit(exec_builder.build())
        .expect_failure();

    builder.write_scratch_to_db();
    builder.flush_environment();

    assert_ne!(
        pre_state_hash,
        builder.get_post_state_hash(),
        "post state hash didn't change..."
    );

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get default account");

    let account1 = builder
        .get_entity_by_account_hash(*ACCOUNT_1_ADDR)
        .expect("should get account 1");

    let account2 = builder
        .get_entity_by_account_hash(*ACCOUNT_2_ADDR)
        .expect("should get account 2");

    let default_account_balance = builder.get_purse_balance(default_account.main_purse());
    let default_expected_balance = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)
        - (MAX_PAYMENT_AMOUNT + TRANSFER_COST + TRANSFER_COST);
    assert_eq!(
        default_account_balance,
        default_expected_balance,
        "default account balance should reflect the transfer ({})",
        U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) - default_expected_balance
    );

    let account_1_balance = builder.get_purse_balance(account1.main_purse());
    assert_eq!(
        account_1_balance,
        U512::zero(),
        "account 1 balance should have been completely consumed"
    );

    let account_2_balance = builder.get_purse_balance(account2.main_purse());
    assert_eq!(
        account_2_balance, *TRANSFER_AMOUNT,
        "account 2 balance should have changed"
    );
}

fn transfer(sender: AccountHash, transfer_args: RuntimeArgs) -> DeployItem {
    let mut rng = rand::thread_rng();
    let deploy_hash = rng.gen();
    DeployItemBuilder::new()
        .with_address(sender)
        .with_empty_payment_bytes(runtime_args! {})
        .with_transfer_args(transfer_args)
        .with_authorization_keys(&[sender])
        .with_deploy_hash(deploy_hash)
        .build()
}
