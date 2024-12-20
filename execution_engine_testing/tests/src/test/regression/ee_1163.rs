use casper_engine_test_support::{
    LmdbWasmTestBuilder, TransferRequestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::Error;
use casper_storage::{data_access_layer::TransferRequest, system::transfer::TransferError};
use casper_types::{
    account::AccountHash, system::handle_payment, Gas, MintCosts, Motes, RuntimeArgs, SystemConfig,
    U512,
};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());
    builder
}

fn should_enforce_limit_for_user_error(
    builder: &mut LmdbWasmTestBuilder,
    request: TransferRequest,
) -> Error {
    let transfer_cost = Gas::from(SystemConfig::default().mint_costs().transfer);

    builder.transfer_and_commit(request);

    let response = builder
        .get_exec_result_owned(0)
        .expect("should have result");

    assert_eq!(response.limit(), transfer_cost);
    assert_eq!(response.consumed(), transfer_cost);

    let handle_payment = builder.get_handle_payment_contract();
    let payment_purse = handle_payment
        .named_keys()
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .expect("should have handle payment payment purse")
        .into_uref()
        .expect("should have uref");
    let payment_purse_balance = builder.get_purse_balance(payment_purse);

    assert_eq!(payment_purse_balance, U512::zero());

    response.error().cloned().expect("should have error")
}

#[ignore]
#[test]
fn should_enforce_system_host_gas_limit() {
    // implies 1:1 gas/motes conversion rate regardless of gas price
    let transfer_amount = Motes::new(U512::one());

    let transfer_request = TransferRequestBuilder::new(transfer_amount.value(), ACCOUNT_1_ADDR)
        .with_initiator(*DEFAULT_ACCOUNT_ADDR)
        .build();

    let mut builder = setup();
    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");
    let main_purse = default_account.main_purse();
    let purse_balance_before = builder.get_purse_balance(main_purse);

    builder
        .transfer_and_commit(transfer_request)
        .expect_success();

    let purse_balance_after = builder.get_purse_balance(main_purse);

    let transfer_cost = Gas::from(MintCosts::default().transfer);
    let response = builder
        .get_exec_result_owned(0)
        .expect("should have result");
    assert_eq!(
        response.limit(),
        transfer_cost,
        "expected actual limit is {}",
        transfer_cost
    );
    assert_eq!(
        purse_balance_before - transfer_amount.value(),
        purse_balance_after
    );
}

#[ignore]
#[test]
fn should_detect_wasmless_transfer_missing_args() {
    let transfer_args = RuntimeArgs::new();
    let transfer_request = TransferRequestBuilder::new(1, AccountHash::default())
        .with_args(transfer_args)
        .build();

    let mut builder = setup();
    let error = should_enforce_limit_for_user_error(&mut builder, transfer_request);

    assert!(matches!(
        error,
        Error::Transfer(TransferError::MissingArgument)
    ));
}

#[ignore]
#[test]
fn should_detect_wasmless_transfer_invalid_purse() {
    let mut builder = setup();
    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");
    let main_purse = default_account.main_purse();

    let transfer_request = TransferRequestBuilder::new(1, main_purse).build();

    let error = should_enforce_limit_for_user_error(&mut builder, transfer_request);
    assert!(matches!(
        error,
        Error::Transfer(TransferError::InvalidPurse)
    ));
}
