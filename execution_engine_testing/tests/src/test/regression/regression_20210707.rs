use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_PAYMENT,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    AccountHash, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{
    core::{
        engine_state::{Error as CoreError, ExecuteRequest},
        execution::Error as ExecError,
    },
    shared::{account::Account, wasm},
};
use casper_types::{
    runtime_args, system::mint, AccessRights, ContractHash, PublicKey, RuntimeArgs, SecretKey,
    URef, U512,
};
use once_cell::sync::Lazy;

const HARDCODED_UREF: URef = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
const CONTRACT_HASH_NAME: &str = "contract_hash";

const METHOD_SEND_TO_ACCOUNT: &str = "send_to_account";
const METHOD_SEND_TO_PURSE: &str = "send_to_purse";
const METHOD_HARDCODED_PURSE_SRC: &str = "hardcoded_purse_src";
const METHOD_STORED_PAYMENT: &str = "stored_payment";
const METHOD_HARDCODED_PAYMENT: &str = "hardcoded_payment";

const ARG_SOURCE: &str = "source";
const ARG_RECIPIENT: &str = "recipient";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";
const ARG_REFUND_PURSE: &str = "refund_purse";

const REGRESSION_20210707: &str = "regression_20210707.wasm";

static ALICE_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ALICE_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ALICE_KEY));

static BOB_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([4; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static BOB_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*BOB_KEY));

fn setup_regression_contract() -> ExecuteRequest {
    ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20210707,
        RuntimeArgs::default(),
    )
    .build()
}

fn transfer(sender: AccountHash, target: AccountHash, amount: u64) -> ExecuteRequest {
    ExecuteRequestBuilder::transfer(
        sender,
        runtime_args! {
            mint::ARG_TARGET => target,
            mint::ARG_AMOUNT => U512::from(amount),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build()
}

fn get_account_contract_hash(account: &Account) -> ContractHash {
    account
        .named_keys()
        .get(CONTRACT_HASH_NAME)
        .cloned()
        .expect("should have contract hash")
        .into_hash()
        .map(ContractHash::new)
        .unwrap()
}

fn assert_forged_uref_error(error: CoreError, forged_uref: URef) {
    assert!(
        matches!(error, CoreError::Exec(ExecError::ForgedReference(uref)) if uref == forged_uref),
        "{:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_transfer_funds_from_forged_purse_to_account() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *ALICE_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request).commit().expect_success();

    let account = builder.get_expected_account(*DEFAULT_ACCOUNT_ADDR);

    let take_from = builder.get_expected_account(*ALICE_ADDR);
    let alice_main_purse = take_from.main_purse();

    let contract_hash = get_account_contract_hash(&account);

    let call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        METHOD_SEND_TO_ACCOUNT,
        runtime_args! {
            ARG_SOURCE => alice_main_purse,
            ARG_RECIPIENT => *BOB_ADDR,
            ARG_AMOUNT => U512::from(700_000_000_000u64),
        },
    )
    .build();

    builder.exec(call_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, alice_main_purse);
}

#[ignore]
#[test]
fn should_not_transfer_funds_from_forged_purse_to_account_native_transfer() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *ALICE_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request).commit().expect_success();

    let take_from = builder.get_expected_account(*ALICE_ADDR);
    let alice_main_purse = take_from.main_purse();

    let transfer_request = {
        let id: Option<u64> = None;
        let transfer_args = runtime_args! {
            mint::ARG_SOURCE => alice_main_purse,
            mint::ARG_TARGET => *BOB_ADDR,
            mint::ARG_AMOUNT => U512::from(700_000_000_000u64),
            mint::ARG_ID => id,
        };

        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args).build()
    };

    builder.exec(transfer_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, alice_main_purse);
}

#[ignore]
#[test]
fn should_not_transfer_funds_from_forged_purse_to_owned_purse() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request_1 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *ALICE_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    let fund_request_2 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *BOB_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request_1).commit().expect_success();
    builder.exec(fund_request_2).commit().expect_success();

    let account = builder.get_expected_account(*DEFAULT_ACCOUNT_ADDR);

    let alice = builder.get_expected_account(*ALICE_ADDR);
    let alice_main_purse = alice.main_purse();

    let bob = builder.get_expected_account(*BOB_ADDR);
    let bob_main_purse = bob.main_purse();

    let contract_hash = get_account_contract_hash(&account);

    let call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        METHOD_SEND_TO_PURSE,
        runtime_args! {
            ARG_SOURCE => alice_main_purse,
            ARG_TARGET => bob_main_purse,
            ARG_AMOUNT => U512::from(700_000_000_000u64),
        },
    )
    .build();

    builder.exec(call_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, alice_main_purse);
}

#[ignore]
#[test]
fn should_not_transfer_funds_into_bob_purse() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request_1 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *BOB_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request_1).commit().expect_success();

    let account = builder.get_expected_account(*DEFAULT_ACCOUNT_ADDR);
    let account_main_purse = account.main_purse();

    let bob = builder.get_expected_account(*BOB_ADDR);
    let bob_main_purse = bob.main_purse();

    let contract_hash = get_account_contract_hash(&account);

    let call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        METHOD_SEND_TO_PURSE,
        runtime_args! {
            ARG_SOURCE => account_main_purse,
            ARG_TARGET => bob_main_purse,
            ARG_AMOUNT => U512::from(700_000_000_000u64),
        },
    )
    .build();

    builder.exec(call_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, bob_main_purse);
}

#[ignore]
#[test]
fn should_not_transfer_from_hardcoded_purse() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request_1 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *BOB_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request_1).commit().expect_success();

    let account = builder.get_expected_account(*DEFAULT_ACCOUNT_ADDR);

    let bob = builder.get_expected_account(*BOB_ADDR);
    let bob_main_purse = bob.main_purse();

    let contract_hash = get_account_contract_hash(&account);

    let call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        METHOD_HARDCODED_PURSE_SRC,
        runtime_args! {
            ARG_TARGET => bob_main_purse,
            ARG_AMOUNT => U512::from(700_000_000_000u64),
        },
    )
    .build();

    builder.exec(call_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, HARDCODED_UREF);
}

#[ignore]
#[test]
fn should_not_refund_to_bob_and_charge_alice() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request_1 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *ALICE_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    let fund_request_2 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *BOB_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request_1).commit().expect_success();
    builder.exec(fund_request_2).commit().expect_success();

    let account = builder.get_expected_account(*DEFAULT_ACCOUNT_ADDR);

    let alice = builder.get_expected_account(*ALICE_ADDR);
    let alice_main_purse = alice.main_purse();

    let bob = builder.get_expected_account(*BOB_ADDR);
    let bob_main_purse = bob.main_purse();

    let contract_hash = get_account_contract_hash(&account);

    let call_request = {
        let args = runtime_args! {
            ARG_REFUND_PURSE => alice_main_purse,
            ARG_SOURCE => bob_main_purse,
            ARG_AMOUNT => *DEFAULT_PAYMENT,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            // Just do nothing if ever we'd get into session execution
            .with_session_bytes(wasm::do_nothing_bytes(), RuntimeArgs::default())
            .with_stored_payment_hash(contract_hash, METHOD_STORED_PAYMENT, args)
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([77; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(call_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, alice_main_purse);
}

#[ignore]
#[test]
fn should_not_charge_alice_for_execution() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request_1 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *ALICE_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    let fund_request_2 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *BOB_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request_1).commit().expect_success();
    builder.exec(fund_request_2).commit().expect_success();

    let account = builder.get_expected_account(*DEFAULT_ACCOUNT_ADDR);
    let account_main_purse = account.main_purse();

    let bob = builder.get_expected_account(*BOB_ADDR);
    let bob_main_purse = bob.main_purse();

    let contract_hash = get_account_contract_hash(&account);

    let call_request = {
        let args = runtime_args! {
            ARG_REFUND_PURSE => account_main_purse,
            ARG_SOURCE => bob_main_purse,
            ARG_AMOUNT => *DEFAULT_PAYMENT,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            // Just do nothing if ever we'd get into session execution
            .with_session_bytes(wasm::do_nothing_bytes(), RuntimeArgs::default())
            .with_stored_payment_hash(contract_hash, METHOD_STORED_PAYMENT, args)
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([77; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(call_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, bob_main_purse);
}

#[ignore]
#[test]
fn should_not_charge_for_execution_from_hardcoded_purse() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let store_request = setup_regression_contract();

    let fund_request_1 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *ALICE_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    let fund_request_2 = transfer(
        *DEFAULT_ACCOUNT_ADDR,
        *BOB_ADDR,
        MINIMUM_ACCOUNT_CREATION_BALANCE,
    );

    builder.exec(store_request).commit().expect_success();
    builder.exec(fund_request_1).commit().expect_success();
    builder.exec(fund_request_2).commit().expect_success();

    let account = builder.get_expected_account(*DEFAULT_ACCOUNT_ADDR);
    let account_main_purse = account.main_purse();

    let contract_hash = get_account_contract_hash(&account);

    let call_request = {
        let args = runtime_args! {
            ARG_REFUND_PURSE => account_main_purse,
            ARG_AMOUNT => *DEFAULT_PAYMENT,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            // Just do nothing if ever we'd get into session execution
            .with_session_bytes(wasm::do_nothing_bytes(), RuntimeArgs::default())
            .with_stored_payment_hash(contract_hash, METHOD_HARDCODED_PAYMENT, args)
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([77; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(call_request).commit();

    let error = builder.get_error().expect("should have error");

    assert_forged_uref_error(error, HARDCODED_UREF);
}
