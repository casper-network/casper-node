use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state, execution};
use casper_types::{account::AccountHash, runtime_args, system::mint, AccessRights, URef, U512};

const TRANSFER_TO_NAMED_PURSE_CONTRACT: &str = "transfer_to_named_purse.wasm";

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const ARG_PURSE_NAME: &str = "purse_name";
const ARG_AMOUNT: &str = "amount";
const DEFAULT_PURSE_BALANCE: u64 = 1_000_000_000;
const PURSE_1: &str = "purse_1";
const PURSE_2: &str = "purse_2";
const ACCOUNT_1_PURSE: &str = "purse_3";

#[ignore]
#[test]
fn regression_20220217_transfer_mint_by_hash_from_main_purse() {
    let mut builder = setup();

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let default_purse = default_account.main_purse();
    let account_1 = builder
        .get_entity_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");
    let account_1_purse = account_1.main_purse();

    let mint_hash = builder.get_mint_contract_hash();

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        ACCOUNT_1_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(ACCOUNT_1_ADDR),
            mint::ARG_SOURCE => default_purse,
            mint::ARG_TARGET => account_1_purse,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => Some(1u64),
        },
    )
    .build();
    builder.exec(exec_request_2).expect_failure().commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == default_purse.with_access_rights(AccessRights::READ_ADD_WRITE),
        ),
        "Expected {:?} revert but received {:?}",
        default_purse,
        error
    );
}

#[ignore]
#[test]
fn regression_20220217_transfer_mint_by_package_hash_from_main_purse() {
    let mut builder = setup();

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let default_purse = default_account.main_purse();
    let account_1 = builder
        .get_entity_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");
    let account_1_purse = account_1.main_purse();

    let mint_hash = builder.get_mint_contract_hash();

    let mint = builder
        .get_addressable_entity(mint_hash)
        .expect("should have mint contract");
    let mint_package_hash = mint.package_hash();

    let exec_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
        ACCOUNT_1_ADDR,
        mint_package_hash,
        None,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(ACCOUNT_1_ADDR),
            mint::ARG_SOURCE => default_purse,
            mint::ARG_TARGET => account_1_purse,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => Some(1u64),
        },
    )
    .build();
    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == default_purse.with_access_rights(AccessRights::READ_ADD_WRITE),
        ),
        "Expected {:?} revert but received {:?}",
        default_purse,
        error
    );
}

#[ignore]
#[test]
fn regression_20220217_mint_by_hash_transfer_from_other_purse() {
    let mut builder = setup();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let purse_1 = account
        .named_keys()
        .get(PURSE_1)
        .unwrap()
        .into_uref()
        .expect("should have purse 1");
    let purse_2 = account
        .named_keys()
        .get(PURSE_2)
        .unwrap()
        .into_uref()
        .expect("should have purse 2");

    let mint_hash = builder.get_mint_contract_hash();

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(ACCOUNT_1_ADDR),
            mint::ARG_SOURCE => purse_1,
            mint::ARG_TARGET => purse_2,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => Some(1u64),
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();
}

#[ignore]
#[test]
fn regression_20220217_mint_by_hash_transfer_from_someones_purse() {
    let mut builder = setup();

    let account_1 = builder
        .get_entity_with_named_keys_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");
    let account_1_purse = account_1
        .named_keys()
        .get(ACCOUNT_1_PURSE)
        .unwrap()
        .into_uref()
        .expect("should have account main purse");

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let purse_1 = default_account
        .named_keys()
        .get(PURSE_1)
        .unwrap()
        .into_uref()
        .expect("should have purse 1");

    let mint_hash = builder.get_mint_contract_hash();

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        ACCOUNT_1_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_SOURCE => purse_1,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_TARGET => account_1_purse,
            mint::ARG_TO => Some(ACCOUNT_1_ADDR),
        },
    )
    .build();

    builder.exec(exec_request).commit();
    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == purse_1
        ),
        "Expected forged uref but received {:?}",
        error,
    );
}

#[ignore]
#[test]
fn regression_20220217_should_not_transfer_funds_on_unrelated_purses() {
    let mut builder = setup();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let purse_1 = account
        .named_keys()
        .get(PURSE_1)
        .unwrap()
        .into_uref()
        .expect("should have purse 1");
    let purse_2 = account
        .named_keys()
        .get(PURSE_2)
        .unwrap()
        .into_uref()
        .expect("should have purse 2");

    let mint_hash = builder.get_mint_contract_hash();

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        ACCOUNT_1_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(*DEFAULT_ACCOUNT_ADDR),
            mint::ARG_SOURCE => purse_1,
            mint::ARG_TARGET => purse_2,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => Some(1u64),
        },
    )
    .build();
    builder.exec(exec_request).expect_failure().commit();

    let error = builder.get_error().expect("should have error");

    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == purse_1
        ),
        "Expected forged uref but received {:?}",
        error,
    );
}

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let fund_account_1_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();
    let fund_purse_1_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        runtime_args! {
            ARG_PURSE_NAME => PURSE_1,
            ARG_AMOUNT => U512::from(DEFAULT_PURSE_BALANCE),
        },
    )
    .build();
    let fund_purse_2_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        runtime_args! {
            ARG_PURSE_NAME => PURSE_2,
            ARG_AMOUNT => U512::from(DEFAULT_PURSE_BALANCE),
        },
    )
    .build();
    let fund_purse_3_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        TRANSFER_TO_NAMED_PURSE_CONTRACT,
        runtime_args! {
            ARG_PURSE_NAME => ACCOUNT_1_PURSE,
            ARG_AMOUNT => U512::from(DEFAULT_PURSE_BALANCE),
        },
    )
    .build();

    builder
        .exec(fund_account_1_request)
        .expect_success()
        .commit();
    builder.exec(fund_purse_1_request).expect_success().commit();
    builder.exec(fund_purse_2_request).expect_success().commit();
    builder.exec(fund_purse_3_request).expect_success().commit();
    builder
}

#[ignore]
#[test]
fn regression_20220217_auction_add_bid_directly() {
    let mut builder = setup();

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let default_purse = default_account.main_purse();
    let account_1 = builder
        .get_entity_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");
    let account_1_purse = account_1.main_purse();

    let mint_hash = builder.get_mint_contract_hash();

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        ACCOUNT_1_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(ACCOUNT_1_ADDR),
            mint::ARG_SOURCE => default_purse,
            mint::ARG_TARGET => account_1_purse,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => Some(1u64),
        },
    )
    .build();
    builder.exec(exec_request_2).expect_failure().commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == default_purse.with_access_rights(AccessRights::READ_ADD_WRITE),
        ),
        "Expected {:?} revert but received {:?}",
        default_purse,
        error
    );
}

#[ignore]
#[test]
fn regression_20220217_() {
    let mut builder = setup();

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let default_purse = default_account.main_purse();
    let account_1 = builder
        .get_entity_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");
    let account_1_purse = account_1.main_purse();

    let mint_hash = builder.get_mint_contract_hash();

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        ACCOUNT_1_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(ACCOUNT_1_ADDR),
            mint::ARG_SOURCE => default_purse,
            mint::ARG_TARGET => account_1_purse,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => Some(1u64),
        },
    )
    .build();
    builder.exec(exec_request_2).expect_failure().commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == default_purse.with_access_rights(AccessRights::READ_ADD_WRITE),
        ),
        "Expected {:?} revert but received {:?}",
        default_purse,
        error
    );
}

#[ignore]
#[test]
fn mint_by_hash_transfer_should_fail_because_lack_of_target_uref_access() {
    // TODO create two named purses and verify we can pass source and target known non-main purses
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let mint_hash = builder.get_mint_contract_hash();

    let source = default_account.main_purse();
    // This could be any URef to which the caller has no access rights.
    let target = URef::default();

    let id = Some(0u64);

    let transfer_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(*DEFAULT_ACCOUNT_ADDR),
            mint::ARG_SOURCE => source,
            mint::ARG_TARGET => target,
            mint::ARG_AMOUNT => U512::one(),
            mint::ARG_ID => id,
        },
    )
    .build();

    // Previously we would allow deposit in this flow to a purse without explicit ADD access. We
    // still allow that in some other flows, but due to code complexity, this is no longer
    // supported.
    builder.exec(transfer_request).expect_failure();
}
