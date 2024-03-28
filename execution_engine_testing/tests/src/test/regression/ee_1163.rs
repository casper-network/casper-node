use casper_engine_test_support::{
    LmdbWasmTestBuilder, TransferRequestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::{Error, WASMLESS_TRANSFER_FIXED_GAS_PRICE};
use casper_storage::{data_access_layer::TransferRequest, system::transfer::TransferError};
use casper_types::{
    account::AccountHash, system::handle_payment, Gas, Motes, RuntimeArgs, SystemConfig, U512,
};

// const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());
    builder
}

fn should_charge_for_user_error(
    builder: &mut LmdbWasmTestBuilder,
    request: TransferRequest,
) -> Error {
    let transfer_cost = Gas::from(SystemConfig::default().mint_costs().transfer);
    let transfer_cost_motes =
        Motes::from_gas(transfer_cost, WASMLESS_TRANSFER_FIXED_GAS_PRICE).expect("gas overflow");

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");
    let main_purse = default_account.main_purse();
    let purse_balance_before = builder.get_purse_balance(main_purse);
    let proposer_purse_balance_before = builder.get_proposer_purse_balance();

    builder.transfer_and_commit(request);

    let purse_balance_after = builder.get_purse_balance(main_purse);
    let proposer_purse_balance_after = builder.get_proposer_purse_balance();

    let response = builder
        .get_exec_result_owned(0)
        .expect("should have result");
    // TODO: reenable when new payment logic is added
    // assert_eq!(response.cost(), transfer_cost);
    assert_eq!(
        purse_balance_before - transfer_cost_motes.value(),
        purse_balance_after
    );
    assert_eq!(
        proposer_purse_balance_before + transfer_cost_motes.value(),
        proposer_purse_balance_after
    );

    // Verify handle payment postconditions

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

// TODO: reenable when new payment logic is added
#[ignore]
#[test]
fn should_properly_charge_fixed_cost_with_nondefault_gas_price() {
    // // implies 1:1 gas/motes conversion rate regardless of gas price
    // let transfer_cost_motes = Motes::new(U512::from(MintCosts::default().transfer));
    //
    // let transfer_amount = Motes::new(U512::one());
    //
    // let id: Option<u64> = None;
    //
    // let transfer_args = runtime_args! {
    //     mint::ARG_TARGET => ACCOUNT_1_ADDR,
    //     mint::ARG_AMOUNT => transfer_amount.value(),
    //     mint::ARG_ID => id,
    // };
    //
    // let transfer_request = {
    //     let deploy_item = DeployItemBuilder::new()
    //         .with_address(*DEFAULT_ACCOUNT_ADDR)
    //         .with_empty_payment_bytes(runtime_args! {})
    //         .with_transfer_args(transfer_args)
    //         .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
    //         .with_deploy_hash([42; 32])
    //         .with_gas_price(PRIORITIZED_GAS_PRICE)
    //         .build();
    //     ExecuteRequestBuilder::from_deploy_item(&deploy_item).build()
    // };
    //
    // let mut builder = setup();
    // let default_account = builder
    //     .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
    //     .expect("should have default account");
    // let main_purse = default_account.main_purse();
    // let purse_balance_before = builder.get_purse_balance(main_purse);
    // let proposer_purse_balance_before = builder.get_proposer_purse_balance();
    //
    // builder.transfer(transfer_request).commit();
    //
    // let purse_balance_after = builder.get_purse_balance(main_purse);
    // let proposer_purse_balance_after = builder.get_proposer_purse_balance();
    //
    // let transfer_cost = Gas::from(MintCosts::default().transfer);
    // let response = builder
    //     .get_exec_result_owned(0)
    //     .expect("should have result")
    //     .get(0)
    //     .cloned()
    //     .expect("should have first result");
    // assert_eq!(
    //     response.cost(),
    //     transfer_cost,
    //     "expected actual cost is {}",
    //     transfer_cost
    // );
    // assert_eq!(
    //     purse_balance_before - transfer_cost_motes.value() - transfer_amount.value(),
    //     purse_balance_after
    // );
    // assert_eq!(
    //     proposer_purse_balance_before + transfer_cost_motes.value(),
    //     proposer_purse_balance_after
    // );
}

#[ignore]
#[test]
fn should_charge_for_wasmless_transfer_missing_args() {
    let transfer_args = RuntimeArgs::new();
    let transfer_request = TransferRequestBuilder::new(1, AccountHash::default())
        .with_args(transfer_args)
        .build();

    let mut builder = setup();
    let error = should_charge_for_user_error(&mut builder, transfer_request);

    assert!(matches!(
        error,
        Error::Transfer(TransferError::MissingArgument)
    ));
}

#[ignore]
#[test]
fn should_charge_for_wasmless_transfer_invalid_purse() {
    let mut builder = setup();
    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");
    let main_purse = default_account.main_purse();

    let transfer_request = TransferRequestBuilder::new(1, main_purse).build();

    let error = should_charge_for_user_error(&mut builder, transfer_request);
    assert!(matches!(
        error,
        Error::Transfer(TransferError::InvalidPurse)
    ));
}
