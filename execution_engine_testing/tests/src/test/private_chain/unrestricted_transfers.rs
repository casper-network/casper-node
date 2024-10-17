use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, TransferRequestBuilder, DEFAULT_PAYMENT,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{engine_state::Error, execution::ExecError};
use casper_storage::system::transfer::TransferError;
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{mint, standard_payment},
    Key, PublicKey, RuntimeArgs, StoredValue, URef, U512,
};

use crate::{test::private_chain::ADMIN_1_ACCOUNT_ADDR, wasm_utils};

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
fn should_restrict_native_transfer_to_from_non_administrators() {
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 =
        TransferRequestBuilder::new(MINIMUM_ACCOUNT_CREATION_BALANCE, *ACCOUNT_1_ADDR)
            .with_initiator(*DEFAULT_ADMIN_ACCOUNT_ADDR)
            .build();

    // Admin can transfer funds to create new account.
    builder
        .transfer_and_commit(fund_transfer_1)
        .expect_success();

    let transfer_request_1 = TransferRequestBuilder::new(1, *ACCOUNT_2_ADDR)
        .with_initiator(*ACCOUNT_1_ADDR)
        .build();

    // User can't transfer funds to non administrator (it doesn't matter if this would create a new
    // account or not...the receiver must be an EXISTING administrator account
    builder
        .transfer_and_commit(transfer_request_1)
        .expect_failure();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            Error::Transfer(TransferError::RestrictedTransferAttempted)
        ),
        "expected RestrictedTransferAttempted error, found {:?}",
        error
    );

    let transfer_request_2 = TransferRequestBuilder::new(1, *DEFAULT_ADMIN_ACCOUNT_ADDR)
        .with_initiator(*ACCOUNT_1_ADDR)
        .build();

    // User can transfer funds back to admin.
    builder
        .transfer_and_commit(transfer_request_2)
        .expect_success();
}

#[ignore]
#[test]
fn should_restrict_wasm_transfer_to_from_non_administrators() {
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
        matches!(error, Error::Exec(ExecError::DisabledUnrestrictedTransfers)),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
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
fn should_noop_self_transfer() {
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
        .get_entity_with_named_keys_by_account_hash(*ACCOUNT_1_ADDR)
        .expect("should have account");
    let maybe_to: Option<AccountHash> = None;
    let source: URef = account.main_purse();
    let target: URef = account
        .named_keys()
        .get(TEST_PURSE)
        .unwrap()
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
    let value: U512 = if let StoredValue::CLValue(cl_value) = value {
        cl_value.into_t().unwrap()
    } else {
        panic!("should be a CLValue");
    };
    assert_eq!(value, U512::zero());
}

#[ignore]
#[test]
fn should_allow_admin_to_native_transfer_from_own_purse() {
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
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have account");
    let maybe_to: Option<AccountHash> = None;
    let source: URef = account.main_purse();
    let target: URef = account
        .named_keys()
        .get(TEST_PURSE)
        .unwrap()
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
    let value: U512 = if let StoredValue::CLValue(cl_value) = value {
        cl_value.into_t().unwrap()
    } else {
        panic!("should be a CLValue");
    };
    assert_eq!(value, amount);
}

#[ignore]
#[test]
fn should_not_allow_wasm_transfer_from_non_administrator_to_misc_purse() {
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
        matches!(
            error,
            Error::Exec(ExecError::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()
        ),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
        error
    )
}

#[ignore]
#[test]
fn should_allow_wasm_transfer_from_administrator() {
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
fn should_not_allow_native_transfer_from_non_administrator_to_misc_purse() {
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
        .get_entity_with_named_keys_by_account_hash(*ACCOUNT_1_ADDR)
        .expect("should have account");
    let source = account.main_purse();
    let target = account
        .named_keys()
        .get(TEST_PURSE)
        .unwrap()
        .into_uref()
        .expect("should be uref");

    let transfer_request = TransferRequestBuilder::new(1, target)
        .with_initiator(*ACCOUNT_1_ADDR)
        .with_source(source)
        .build();

    builder
        .transfer_and_commit(transfer_request)
        .expect_failure();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            Error::Transfer(TransferError::UnableToVerifyTargetIsAdmin)
        ),
        "expected UnableToVerifyTargetIsAdmin error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_allow_native_transfer_to_administrator_from_misc_purse() {
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
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have account");
    let source = account.main_purse();
    let target = account
        .named_keys()
        .get(TEST_PURSE)
        .unwrap()
        .into_uref()
        .expect("should be uref");

    let transfer_request = TransferRequestBuilder::new(1, target)
        .with_initiator(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .with_source(source)
        .build();

    builder
        .transfer_and_commit(transfer_request)
        .expect_success();
}

#[ignore]
#[test]
fn should_not_allow_wasm_transfer_from_non_administrator_to_known_purse() {
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
        matches!(
            error,
            Error::Exec(ExecError::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()
        ),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
        error
    );
}

#[ignore]
#[allow(unused)]
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
    let sender = *ACCOUNT_1_ADDR;
    let deploy_hash = [100; 32];

    let payment_args = runtime_args! {
        standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT,
    };
    let session_args = RuntimeArgs::default();

    const PAY_ENTRYPOINT: &str = "pay";
    let deploy_item = DeployItemBuilder::new()
        .with_address(sender)
        .with_session_bytes(wasm_utils::do_minimum_bytes(), session_args)
        .with_stored_payment_named_key(TEST_PAYMENT_STORED_HASH_NAME, PAY_ENTRYPOINT, payment_args)
        .with_authorization_keys(&[sender])
        .with_deploy_hash(deploy_hash)
        .build();
    let exec_request_1 = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    builder.exec(exec_request_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(ExecError::ForgedReference(_))),
        "expected ForgedReference error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_allow_direct_mint_transfer_with_system_addr_specified() {
    // This test executes mint's transfer entrypoint with a SYSTEM_ADDR as to field in attempt to
    // avoid restrictions.
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        "mint_transfer_proxy.wasm",
        runtime_args! {
            "to" => Some(PublicKey::System.to_account_hash()),
            "amount" => U512::from(1u64),
        },
    )
    .build();

    // should fail because the imputed TO arg is not valid if PublicKey::System in this flow
    builder.exec(fund_transfer_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(ExecError::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_allow_direct_mint_transfer_with_an_admin_in_to_field() {
    // This test executes mint's transfer entrypoint with a SYSTEM_ADDR as to field in attempt to
    // avoid restrictions.
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        "mint_transfer_proxy.wasm",
        runtime_args! {
            "to" => Some(*ADMIN_1_ACCOUNT_ADDR),
            "amount" => U512::from(1u64),
        },
    )
    .build();

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(ExecError::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_allow_mint_transfer_without_to_field_from_admin() {
    // This test executes mint's transfer entrypoint with a SYSTEM_ADDR as to field in attempt to
    // avoid restrictions.
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 = ExecuteRequestBuilder::standard(
        *ADMIN_1_ACCOUNT_ADDR,
        "mint_transfer_proxy.wasm",
        runtime_args! {
            "to" => None::<AccountHash>,
            "amount" => U512::from(1u64),
        },
    )
    .build();

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_allow_transfer_without_to_field_from_non_admin() {
    // This test executes mint's transfer entrypoint with a SYSTEM_ADDR as to field in attempt to
    // avoid restrictions.
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        "mint_transfer_proxy.wasm",
        runtime_args! {
            "to" => None::<AccountHash>,
            "amount" => U512::from(1u64),
        },
    )
    .build();

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(ExecError::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
        error
    );
}

// #[ignore]
// #[allow(unused)]
// #[test]
// fn should_not_allow_custom_payment() {
//     let mut builder = super::private_chain_setup();
//
//     // Account 1 can deploy after genesis
//     let sender = *ACCOUNT_1_ADDR;
//     let deploy_hash = [100; 32];
//
//     let payment_amount = *DEFAULT_PAYMENT + U512::from(1u64);
//
//     let payment_args = runtime_args! {
//         standard_payment::ARG_AMOUNT => payment_amount,
//     };
//     let session_args = RuntimeArgs::default();
//
//     let deploy_item = DeployItemBuilder::new()
//         .with_address(sender)
//         .with_session_bytes(wasm_utils::do_minimum_bytes(), session_args)
//         .with_payment_code("non_standard_payment.wasm", payment_args)
//         .with_authorization_keys(&[sender])
//         .with_deploy_hash(deploy_hash)
//         .build();
//     let exec_request_1 = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();
//
//     builder.exec(exec_request_1).expect_failure();
// }
//
// #[ignore]
// #[test]
// fn should_allow_wasm_transfer_to_system() {
//     let mut builder = super::private_chain_setup();
//
//     // Account 1 can deploy after genesis
//     let sender = *ACCOUNT_1_ADDR;
//     let deploy_hash = [100; 32];
//
//     let payment_amount = *DEFAULT_PAYMENT + U512::from(1u64);
//
//     let payment_args = runtime_args! {
//         standard_payment::ARG_AMOUNT => payment_amount,
//     };
//     let session_args = runtime_args! {
//         "target" => *SYSTEM_ADDR,
//         "amount" => U512::one(),
//     };
//
//     let deploy_item = DeployItemBuilder::new()
//         .with_address(sender)
//         .with_session_code("transfer_to_account_u512.wasm", session_args)
//         .with_standard_payment(payment_args)
//         .with_authorization_keys(&[sender])
//         .with_deploy_hash(deploy_hash)
//         .build();
//     let exec_request_1 = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();
//
//     builder.exec(exec_request_1).expect_success().commit();
//
//     let handle_payment_contract = builder.get_named_keys(EntityAddr::System(
//         builder.get_handle_payment_contract_hash().value(),
//     ));
//     let payment_purse_key = handle_payment_contract
//         .get(handle_payment::PAYMENT_PURSE_KEY)
//         .unwrap();
//     let payment_purse_uref = payment_purse_key.into_uref().unwrap();
//     println!("payment uref: {payment_purse_uref}");
//     assert_eq!(
//         builder.get_purse_balance(payment_purse_uref),
//         U512::zero(),
//         "after finalizing a private chain a payment purse should be empty"
//     );
// }
//
// #[ignore]
// #[test]
// fn should_allow_native_transfer_to_administrator() {
//     let mut builder = super::private_chain_setup();
//
//     let payment_purse_uref = {
//         let handle_payment_contract = builder.get_named_keys(EntityAddr::System(
//             builder.get_handle_payment_contract_hash().value(),
//         ));
//         let payment_purse_key = handle_payment_contract
//             .get(handle_payment::PAYMENT_PURSE_KEY)
//             .unwrap();
//         payment_purse_key.into_uref().unwrap()
//     };
//
//     assert_eq!(
//         builder.get_purse_balance(payment_purse_uref),
//         U512::zero(),
//         "payment purse should be empty"
//     );
//
//     let fund_transfer_1 =
//         TransferRequestBuilder::new(MINIMUM_ACCOUNT_CREATION_BALANCE, *SYSTEM_ADDR)
//             .with_initiator(*DEFAULT_ADMIN_ACCOUNT_ADDR)
//             .build();
//
//     builder
//         .transfer_and_commit(fund_transfer_1)
//         .expect_success();
//
//     assert_eq!(
//         builder.get_purse_balance(payment_purse_uref),
//         U512::zero(),
//         "after finalizing a private chain a payment purse should be empty"
//     );
// }
