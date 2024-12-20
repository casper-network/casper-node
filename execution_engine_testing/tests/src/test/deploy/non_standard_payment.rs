use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, DEFAULT_PROTOCOL_VERSION, LOCAL_GENESIS_REQUEST,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{engine_state::BlockInfo, execution::ExecError};
use casper_storage::data_access_layer::BalanceIdentifier;
use casper_types::{
    account::AccountHash, runtime_args, ApiError, BlockHash, Digest, Gas, RuntimeArgs, Timestamp,
    U512,
};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);
const DO_NOTHING_WASM: &str = "do_nothing.wasm";
const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_MAIN_PURSE_TO_NEW_PURSE_WASM: &str = "transfer_main_purse_to_new_purse.wasm";
const PAYMENT_PURSE_PERSIST_WASM: &str = "payment_purse_persist.wasm";
const NAMED_PURSE_PAYMENT_WASM: &str = "named_purse_payment.wasm";
const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";
const ARG_PURSE_NAME: &str = "purse_name";
const ARG_DESTINATION: &str = "destination";

#[ignore]
#[test]
fn should_charge_non_main_purse() {
    // as account_1, create & fund a new purse and use that to pay for something
    // instead of account_1 main purse
    const TEST_PURSE_NAME: &str = "test-purse";

    let account_1_account_hash = ACCOUNT_1_ADDR;
    let account_1_funding_amount = U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE);
    let account_1_purse_funding_amount = *DEFAULT_PAYMENT;

    let mut builder = LmdbWasmTestBuilder::default();

    let setup_exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => account_1_funding_amount },
    )
    .build();

    let create_purse_exec_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        TRANSFER_MAIN_PURSE_TO_NEW_PURSE_WASM,
        runtime_args! { ARG_DESTINATION => TEST_PURSE_NAME, ARG_AMOUNT => account_1_purse_funding_amount },
    )
        .build();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    builder
        .exec(setup_exec_request)
        .expect_success()
        .commit()
        .exec(create_purse_exec_request)
        .expect_success()
        .commit();

    // get account_1
    let account_1 = builder
        .get_entity_with_named_keys_by_account_hash(ACCOUNT_1_ADDR)
        .expect("should have account");
    // get purse
    let purse_key = account_1.named_keys().get(TEST_PURSE_NAME).unwrap();
    let purse = purse_key.into_uref().expect("should have uref");
    let purse_starting_balance = builder.get_purse_balance(purse);

    assert_eq!(
        purse_starting_balance, account_1_purse_funding_amount,
        "purse should be funded with expected amount, which in this case is also == to the amount to be paid"
    );

    // in this test, we're just going to pay everything in the purse to
    // keep the math easy.
    let amount_to_be_paid = account_1_purse_funding_amount;
    // should be able to pay for exec using new purse
    let deploy_item = DeployItemBuilder::new()
        .with_address(ACCOUNT_1_ADDR)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
        .with_payment_code(
            NAMED_PURSE_PAYMENT_WASM,
            runtime_args! {
                ARG_PURSE_NAME => TEST_PURSE_NAME,
                ARG_AMOUNT => amount_to_be_paid
            },
        )
        .with_authorization_keys(&[account_1_account_hash])
        .with_deploy_hash([3; 32])
        .build();

    let block_time = Timestamp::now().millis();
    let parent_block_hash = BlockHash::default();
    let block_info = BlockInfo::new(Digest::default(), block_time.into(), parent_block_hash, 1);
    builder
        .exec_wasm_v1(
            deploy_item
                .new_custom_payment_from_deploy_item(block_info, Gas::from(12_500_000_000_u64))
                .expect("should be valid req"),
        )
        .expect_success()
        .commit();

    let payment_purse_balance = builder
        .get_purse_balance_result_with_proofs(DEFAULT_PROTOCOL_VERSION, BalanceIdentifier::Payment);

    assert!(
        payment_purse_balance.is_success(),
        "payment purse balance check should succeed"
    );

    let paid_amount = *payment_purse_balance
        .available_balance()
        .expect("should have payment amount");

    assert_eq!(
        paid_amount, amount_to_be_paid,
        "purse resting balance should equal funding amount minus exec costs"
    );

    let purse_final_balance = builder.get_purse_balance(purse);

    assert_eq!(
        purse_final_balance,
        U512::zero(),
        "since we zero'd out the paying purse, the final balance should be zero"
    );
}

const ARG_METHOD: &str = "method";

#[ignore]
#[test]
fn should_not_allow_custom_payment_purse_persistence_1() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let account_hash = *DEFAULT_ACCOUNT_ADDR;

    let deploy_item = DeployItemBuilder::new()
        .with_address(account_hash)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
        .with_payment_code(
            PAYMENT_PURSE_PERSIST_WASM,
            runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT, ARG_METHOD => "put_key"},
        )
        .with_deploy_hash([1; 32])
        .with_authorization_keys(&[account_hash])
        .build();
    let block_info = BlockInfo::new(
        Digest::default(),
        Timestamp::now().millis().into(),
        BlockHash::default(),
        1,
    );
    let limit = Gas::from(12_500_000_000_u64);

    let request = deploy_item
        .new_custom_payment_from_deploy_item(block_info, limit)
        .expect("should be valid req");

    builder.exec_wasm_v1(request).expect_failure();

    builder.assert_error(casper_execution_engine::engine_state::Error::Exec(
        ExecError::Revert(ApiError::HandlePayment(40)),
    ));
}

#[ignore]
#[test]
fn should_not_allow_custom_payment_purse_persistence_2() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let account_hash = *DEFAULT_ACCOUNT_ADDR;

    let deploy_item = DeployItemBuilder::new()
        .with_address(account_hash)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
        .with_payment_code(
            PAYMENT_PURSE_PERSIST_WASM,
            runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT, ARG_METHOD => "call_contract"},
        )
        .with_deploy_hash([1; 32])
        .with_authorization_keys(&[account_hash])
        .build();
    let block_info = BlockInfo::new(
        Digest::default(),
        Timestamp::now().millis().into(),
        BlockHash::default(),
        1,
    );
    let limit = Gas::from(12_500_000_000_u64);

    let request = deploy_item
        .new_custom_payment_from_deploy_item(block_info, limit)
        .expect("should be valid req");

    builder.exec_wasm_v1(request).expect_failure();

    builder.assert_error(casper_execution_engine::engine_state::Error::Exec(
        ExecError::Revert(ApiError::HandlePayment(40)),
    ));
}

#[ignore]
#[test]
fn should_not_allow_custom_payment_purse_persistence_3() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let account_hash = *DEFAULT_ACCOUNT_ADDR;

    let deploy_item = DeployItemBuilder::new()
        .with_address(account_hash)
        .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
        .with_payment_code(
            PAYMENT_PURSE_PERSIST_WASM,
            runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT, ARG_METHOD => "call_versioned_contract"},
        )
        .with_deploy_hash([1; 32])
        .with_authorization_keys(&[account_hash])
        .build();
    let block_info = BlockInfo::new(
        Digest::default(),
        Timestamp::now().millis().into(),
        BlockHash::default(),
        1,
    );
    let limit = Gas::from(12_500_000_000_u64);

    let request = deploy_item
        .new_custom_payment_from_deploy_item(block_info, limit)
        .expect("should be valid req");

    builder.exec_wasm_v1(request).expect_failure();

    builder.assert_error(casper_execution_engine::engine_state::Error::Exec(
        ExecError::Revert(ApiError::HandlePayment(40)),
    ));
}
