use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{engine_state::Error, execution};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{mint, standard_payment},
    Key, RuntimeArgs, URef, U512,
};

use crate::wasm_utils;

use super::{ACCOUNT_1_ADDR, ACCOUNT_2_ADDR, DEFAULT_ADMIN_ACCOUNT_ADDR};

const TRANSFER_TO_ACCOUNT_U512_CONTRACT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_TO_NAMED_PURSE_CONTRACT: &str = "transfer_to_named_purse.wasm";

const TEST_PURSE: &str = "test";
const ARG_PURSE_NAME: &str = "purse_name";
const ARG_AMOUNT: &str = "amount";

const TEST_PAYMENT_STORED_CONTRACT: &str = "test_payment_stored.wasm";
const TEST_PAYMENT_STORED_HASH_NAME: &str = "test_payment_hash";

#[ignore]
#[test]
fn should_disallow_native_p2p_transfer_to_create_new_account_by_user() {
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 = ExecuteRequestBuilder::transfer(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => *ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_success().commit();

    let transfer_request_1 = ExecuteRequestBuilder::transfer(
        *ACCOUNT_1_ADDR,
        runtime_args! {
            mint::ARG_TARGET => *ACCOUNT_2_ADDR,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    // User can't transfer funds to create new account.
    builder.exec(transfer_request_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::DisabledP2PTransfers)),
        "expected DisabledP2PTransfers error, found {:?}",
        error
    );

    let transfer_request_2 = ExecuteRequestBuilder::transfer(
        *ACCOUNT_1_ADDR,
        runtime_args! {
            mint::ARG_TARGET => *DEFAULT_ADMIN_ACCOUNT_ADDR,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    // User can transfer funds back to admin.
    builder.exec(transfer_request_2).expect_success().commit();
}

#[ignore]
#[test]
fn should_disallow_wasm_p2p_transfer_to_create_new_account_by_user() {
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        TRANSFER_TO_ACCOUNT_U512_CONTRACT,
        runtime_args! {
            mint::ARG_TARGET => *ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        },
    )
    .build();

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_success().commit();

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        TRANSFER_TO_ACCOUNT_U512_CONTRACT,
        runtime_args! {
            mint::ARG_TARGET => *ACCOUNT_2_ADDR,
            mint::ARG_AMOUNT => U512::one(),
        },
    )
    .build();

    // User can't transfer funds to create new account.
    builder.exec(transfer_request_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::DisabledP2PTransfers)),
        "expected DisabledP2PTransfers error, found {:?}",
        error
    );

    let transfer_request_2 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        TRANSFER_TO_ACCOUNT_U512_CONTRACT,
        runtime_args! {
            mint::ARG_TARGET => *DEFAULT_ADMIN_ACCOUNT_ADDR,
            mint::ARG_AMOUNT => U512::one(),
        },
    )
    .build();

    // User can transfer funds back to admin.
    builder.exec(transfer_request_2).expect_success().commit();
}

#[ignore]
#[test]
fn should_disallow_transfer_to_own_purse_via_direct_mint_transfer_call() {
    let mut builder = super::private_chain_setup();

    let session_args = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE,
        ARG_AMOUNT => U512::zero(), // create empty purse without transfer
    };
    let create_purse_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        session_args,
    )
    .build();
    builder.exec(create_purse_request).expect_success().commit();

    let mint_contract_hash = builder.get_mint_contract_hash();

    let account = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account");
    let maybe_to: Option<AccountHash> = None;
    let source: URef = account.main_purse();
    let target: URef = account.named_keys()[TEST_PURSE]
        .into_uref()
        .expect("should be uref");
    let amount: U512 = U512::one();
    let id: Option<u64> = None;

    let session_args = runtime_args! {
        mint::ARG_TO => maybe_to,
        mint::ARG_SOURCE => source,
        mint::ARG_TARGET => target,
        mint::ARG_AMOUNT => amount,
        mint::ARG_ID => id,
    };

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *ACCOUNT_1_ADDR,
        mint_contract_hash,
        mint::METHOD_TRANSFER,
        session_args,
    )
    .build();
    builder.exec(exec_request).expect_success().commit();

    // Transfer technically succeeded but the result of mint::Error was discarded so we have to
    // ensure that purse has 0 balance.
    let value = builder
        .query(None, Key::Balance(target.addr()), &[])
        .unwrap();
    let value_cl = value.into_clvalue().unwrap();
    let value: U512 = value_cl.into_t().unwrap();
    assert_eq!(value, U512::zero());
}

#[ignore]
#[test]
fn should_allow_admin_to_transfer_to_own_purse_via_direct_mint_transfer_call() {
    let mut builder = super::private_chain_setup();

    let session_args = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE,
        ARG_AMOUNT => U512::zero(), // create empty purse without transfer
    };
    let create_purse_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        session_args,
    )
    .build();
    builder.exec(create_purse_request).expect_success().commit();

    let mint_contract_hash = builder.get_mint_contract_hash();

    let account = builder
        .get_account(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have account");
    let maybe_to: Option<AccountHash> = None;
    let source: URef = account.main_purse();
    let target: URef = account.named_keys()[TEST_PURSE]
        .into_uref()
        .expect("should be uref");
    let amount: U512 = U512::one();
    let id: Option<u64> = None;

    let session_args = runtime_args! {
        mint::ARG_TO => maybe_to,
        mint::ARG_SOURCE => source,
        mint::ARG_TARGET => target,
        mint::ARG_AMOUNT => amount,
        mint::ARG_ID => id,
    };

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        mint_contract_hash,
        mint::METHOD_TRANSFER,
        session_args,
    )
    .build();
    builder.exec(exec_request).expect_success().commit();

    // Transfer technically succeeded but the result of mint::Error was discarded so we have to
    // ensure that purse has 0 balance.
    let value = builder
        .query(None, Key::Balance(target.addr()), &[])
        .unwrap();
    let value_cl = value.into_clvalue().unwrap();
    let value: U512 = value_cl.into_t().unwrap();
    assert_eq!(value, amount);
}

#[ignore]
#[test]
fn should_disallow_transfer_to_own_purse_in_wasm_session() {
    let mut builder = super::private_chain_setup();

    let session_args = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE,
        ARG_AMOUNT => U512::one(),
    };
    let create_purse_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        session_args,
    )
    .build();
    builder.exec(create_purse_request).expect_failure().commit();
    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::DisabledP2PTransfers)),
        "expected DisabledP2PTransfers error, found {:?}",
        error
    )
}

#[ignore]
#[test]
fn should_allow_admin_to_transfer_to_own_purse_in_wasm_session() {
    let mut builder = super::private_chain_setup();

    let session_args = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE,
        ARG_AMOUNT => U512::one(),
    };
    let create_purse_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        session_args,
    )
    .build();
    builder.exec(create_purse_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_disallow_transfer_to_own_purse_via_native_transfer() {
    let mut builder = super::private_chain_setup();

    let session_args = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE,
        ARG_AMOUNT => U512::zero(), // we can't transfer in private chain mode, so we'll just create empty valid purse
    };
    let create_purse_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        session_args,
    )
    .build();
    builder.exec(create_purse_request).expect_success().commit();

    let account = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account");
    let source: URef = account.main_purse();
    let target: URef = account.named_keys()[TEST_PURSE]
        .into_uref()
        .expect("should be uref");
    let amount: U512 = U512::one();
    let id: Option<u64> = None;

    let transfer_request = ExecuteRequestBuilder::transfer(
        *ACCOUNT_1_ADDR,
        runtime_args! {
            mint::ARG_SOURCE => source,
            mint::ARG_TARGET => target,
            mint::ARG_AMOUNT => amount,
            mint::ARG_ID => id,
        },
    )
    .build();

    builder.exec(transfer_request).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::DisabledP2PTransfers)),
        "expected DisabledP2PTransfers error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_allow_admin_to_transfer_to_own_purse_via_native_transfer() {
    let mut builder = super::private_chain_setup();

    let session_args = runtime_args! {
        ARG_PURSE_NAME => TEST_PURSE,
        ARG_AMOUNT => U512::zero(), // we can't transfer in private chain mode, so we'll just create empty valid purse
    };
    let create_purse_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        session_args,
    )
    .build();
    builder.exec(create_purse_request).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have account");
    let source: URef = account.main_purse();
    let target: URef = account.named_keys()[TEST_PURSE]
        .into_uref()
        .expect("should be uref");
    let amount: U512 = U512::one();
    let id: Option<u64> = None;

    let transfer_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_SOURCE => source,
            mint::ARG_TARGET => target,
            mint::ARG_AMOUNT => amount,
            mint::ARG_ID => id,
        },
    )
    .build();

    builder.exec(transfer_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_disallow_wasm_payment_to_purse() {
    let mut builder = super::private_chain_setup();

    let store_contract_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        "contract_funds.wasm",
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(store_contract_request)
        .expect_success()
        .commit();

    let transfer_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        "contract_funds_call.wasm",
        runtime_args! {
            ARG_AMOUNT => U512::one(),
        },
    )
    .build();

    builder.exec(transfer_request).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::DisabledP2PTransfers)),
        "expected DisabledP2PTransfers error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_allow_payment_to_purse_in_stored_payment() {
    // This effectively disables any custom payment code
    let mut builder = super::private_chain_setup();

    let store_contract_request = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        TEST_PAYMENT_STORED_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(store_contract_request)
        .expect_success()
        .commit();

    // Account 1 can deploy after genesis
    let exec_request_1 = {
        let sender = *ACCOUNT_1_ADDR;
        let deploy_hash = [100; 32];

        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT,
        };
        let session_args = RuntimeArgs::default();

        const PAY_ENTRYPOINT: &str = "pay";
        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_session_bytes(wasm_utils::do_minimum_bytes(), session_args)
            .with_stored_payment_named_key(
                TEST_PAYMENT_STORED_HASH_NAME,
                PAY_ENTRYPOINT,
                payment_args,
            )
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::DisabledP2PTransfers)),
        "expected DisabledP2PTransfers error, found {:?}",
        error
    );
}
