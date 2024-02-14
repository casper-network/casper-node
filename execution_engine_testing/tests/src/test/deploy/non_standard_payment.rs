use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);
const DO_NOTHING_WASM: &str = "do_nothing.wasm";
const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const TRANSFER_MAIN_PURSE_TO_NEW_PURSE_WASM: &str = "transfer_main_purse_to_new_purse.wasm";
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
    let payment_purse_amount = *DEFAULT_PAYMENT;
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

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

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
        "purse should be funded with expected amount"
    );

    // should be able to pay for exec using new purse
    let account_payment_exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_session_code(DO_NOTHING_WASM, RuntimeArgs::default())
            .with_payment_code(
                NAMED_PURSE_PAYMENT_WASM,
                runtime_args! {
                    ARG_PURSE_NAME => TEST_PURSE_NAME,
                    ARG_AMOUNT => payment_purse_amount
                },
            )
            .with_authorization_keys(&[account_1_account_hash])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder
        .exec(account_payment_exec_request)
        .expect_success()
        .commit();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    let expected_resting_balance = account_1_purse_funding_amount - transaction_fee;

    let purse_final_balance = builder.get_purse_balance(purse);

    assert_eq!(
        purse_final_balance, expected_resting_balance,
        "purse resting balance should equal funding amount minus exec costs"
    );
}
