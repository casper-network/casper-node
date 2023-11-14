use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE,
    SYSTEM_ADDR,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{handle_payment, mint, standard_payment},
    EntityAddr, Key, PublicKey, RuntimeArgs, StoredValue, URef, U512,
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
fn should_disallow_native_unrestricted_transfer_to_create_new_account_by_user() {
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
        matches!(
            error,
            Error::Exec(execution::Error::DisabledUnrestrictedTransfers)
        ),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
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
fn should_disallow_wasm_unrestricted_transfer_to_create_new_account_by_user() {
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
        matches!(
            error,
            Error::Exec(execution::Error::DisabledUnrestrictedTransfers)
        ),
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

    // What is
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
        matches!(
            error,
            Error::Exec(execution::Error::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()
        ),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
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
        .get_entity_with_named_keys_by_account_hash(*ACCOUNT_1_ADDR)
        .expect("should have account");
    let source: URef = account.main_purse();
    let target: URef = account
        .named_keys()
        .get(TEST_PURSE)
        .unwrap()
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
        matches!(
            error,
            Error::Exec(execution::Error::DisabledUnrestrictedTransfers)
        ),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
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
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ADMIN_ACCOUNT_ADDR)
        .expect("should have account");
    let source: URef = account.main_purse();
    let target: URef = account
        .named_keys()
        .get(TEST_PURSE)
        .unwrap()
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
        matches!(
            error,
            Error::Exec(execution::Error::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()
        ),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
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
        matches!(error, Error::Exec(execution::Error::ForgedReference(_))),
        "expected InvalidContext error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_disallow_native_unrestricted_transfer_to_existing_account_by_user() {
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

    let fund_transfer_2 = ExecuteRequestBuilder::transfer(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => *ACCOUNT_2_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_success().commit();
    builder.exec(fund_transfer_2).expect_success().commit();

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
        matches!(
            error,
            Error::Exec(execution::Error::DisabledUnrestrictedTransfers)
        ),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
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
fn should_disallow_wasm_unrestricted_transfer_to_existing_account_by_user() {
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

    let fund_transfer_2 = ExecuteRequestBuilder::transfer(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => *ACCOUNT_2_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_success().commit();
    builder.exec(fund_transfer_2).expect_success().commit();

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
        matches!(
            error,
            Error::Exec(execution::Error::Revert(api_error)) if api_error == mint::Error::DisabledUnrestrictedTransfers.into()),
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

    // Admin can transfer funds to create new account.
    builder.exec(fund_transfer_1).expect_failure().commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, Error::Exec(execution::Error::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()),
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
        matches!(error, Error::Exec(execution::Error::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_not_allow_direct_mint_transfer_without_to_field() {
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
        matches!(error, Error::Exec(execution::Error::Revert(revert)) if revert == mint::Error::DisabledUnrestrictedTransfers.into()),
        "expected DisabledUnrestrictedTransfers error, found {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_allow_custom_payment_by_paying_to_system_account() {
    let mut builder = super::private_chain_setup();

    // Account 1 can deploy after genesis
    let exec_request_1 = {
        let sender = *ACCOUNT_1_ADDR;
        let deploy_hash = [100; 32];

        let payment_amount = *DEFAULT_PAYMENT + U512::from(1u64);

        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => payment_amount,
        };
        let session_args = RuntimeArgs::default();

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_session_bytes(wasm_utils::do_minimum_bytes(), session_args)
            .with_payment_code("non_standard_payment.wasm", payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_1).expect_success().commit();

    let handle_payment_contract = builder.get_named_keys(EntityAddr::System(
        builder.get_handle_payment_contract_hash().value(),
    ));
    let payment_purse_key = handle_payment_contract
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .unwrap();
    let payment_purse_uref = payment_purse_key.into_uref().unwrap();
    assert_eq!(
        builder.get_purse_balance(payment_purse_uref),
        U512::zero(),
        "after finalizing a private chain custom payment code a payment purse should be empty"
    );
}

#[ignore]
#[test]
fn should_allow_transfer_to_system_in_a_session_code() {
    let mut builder = super::private_chain_setup();

    // Account 1 can deploy after genesis
    let exec_request_1 = {
        let sender = *ACCOUNT_1_ADDR;
        let deploy_hash = [100; 32];

        let payment_amount = *DEFAULT_PAYMENT + U512::from(1u64);

        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => payment_amount,
        };
        let session_args = runtime_args! {
            "target" => *SYSTEM_ADDR,
            "amount" => U512::one(),
        };

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_session_code("transfer_to_account_u512.wasm", session_args)
            .with_payment_bytes(Vec::new(), payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_1).expect_success().commit();

    let handle_payment_contract = builder.get_named_keys(EntityAddr::System(
        builder.get_handle_payment_contract_hash().value(),
    ));
    let payment_purse_key = handle_payment_contract
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .unwrap();
    let payment_purse_uref = payment_purse_key.into_uref().unwrap();
    assert_eq!(
        builder.get_purse_balance(payment_purse_uref),
        U512::zero(),
        "after finalizing a private chain custom payment code a payment purse should be empty"
    );

    let system_account = builder.get_entity_by_account_hash(*SYSTEM_ADDR).unwrap();
    assert_eq!(
        system_account.main_purse().addr(),
        payment_purse_uref.addr()
    );
}

#[ignore]
#[test]
fn should_allow_transfer_to_system_in_a_native_transfer() {
    let mut builder = super::private_chain_setup();

    let fund_transfer_1 = ExecuteRequestBuilder::transfer(
        *DEFAULT_ADMIN_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => *SYSTEM_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    builder.exec(fund_transfer_1).expect_success().commit();

    let handle_payment_contract = builder.get_named_keys(EntityAddr::System(
        builder.get_handle_payment_contract_hash().value(),
    ));
    let payment_purse_key = handle_payment_contract
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .unwrap();
    let payment_purse_uref = payment_purse_key.into_uref().unwrap();
    assert_eq!(
        builder.get_purse_balance(payment_purse_uref),
        U512::zero(),
        "after finalizing a private chain custom payment code a payment purse should be empty"
    );

    let system_account = builder.get_entity_by_account_hash(*SYSTEM_ADDR).unwrap();
    assert_eq!(
        system_account.main_purse().addr(),
        payment_purse_uref.addr()
    );
}
