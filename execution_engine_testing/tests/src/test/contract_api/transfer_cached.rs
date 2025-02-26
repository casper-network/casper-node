use once_cell::sync::Lazy;
use tempfile::TempDir;

use casper_engine_test_support::{
    LmdbWasmTestBuilder, TransferRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, LOCAL_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, MintCosts, PublicKey, SecretKey, U512};

/// The maximum amount of motes that payment code execution can cost.
const TRANSFER_MOTES_AMOUNT: u64 = 2_500_000_000;

static TRANSFER_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(TRANSFER_MOTES_AMOUNT));

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

#[ignore]
#[test]
fn should_transfer_to_account() {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.path());

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let pre_state_hash = builder.get_post_state_hash();

    // Default account to account 1
    let transfer_request = TransferRequestBuilder::new(1, *ACCOUNT_1_ADDR).build();
    builder
        .transfer_and_commit(transfer_request)
        .expect_success();

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
    let default_expected_balance = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) - (U512::one());
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
fn should_transfer_multiple_times() {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.path());

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let pre_state_hash = builder.get_post_state_hash();

    // Default account to account 1
    // We must first transfer the amount account 1 will transfer to account 2, along with the fee
    // account 1 will need to pay for that transfer.
    let transfer_request = TransferRequestBuilder::new(
        *TRANSFER_AMOUNT + MintCosts::default().transfer,
        *ACCOUNT_1_ADDR,
    )
    .build();
    builder
        .transfer_and_commit(transfer_request)
        .expect_success();

    let transfer_request = TransferRequestBuilder::new(*TRANSFER_AMOUNT, *ACCOUNT_2_ADDR)
        .with_initiator(*ACCOUNT_1_ADDR)
        .build();
    builder
        .transfer_and_commit(transfer_request)
        .expect_success();

    // Double spend test for account 1
    let transfer_request = TransferRequestBuilder::new(*TRANSFER_AMOUNT, *ACCOUNT_2_ADDR)
        .with_initiator(*ACCOUNT_1_ADDR)
        .build();
    builder
        .transfer_and_commit(transfer_request)
        .expect_failure();

    assert_ne!(
        pre_state_hash,
        builder.get_post_state_hash(),
        "post state hash didn't change..."
    );
}
