use casper_engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder,
        DEFAULT_ACCOUNT_KEY, DEFAULT_GAS_PRICE, DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casper_execution_engine::shared::motes::Motes;
use casper_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const CONTRACT_TRANSFER_TO_ACCOUNT_NAME: &str = "transfer_to_account";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const TRANSFER_ENTRYPOINT: &str = "transfer";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

#[ignore]
#[test]
fn should_transfer_to_account_stored() {
    let mut builder = InMemoryWasmTestBuilder::default();
    {
        // first, store transfer contract
        let exec_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            &format!("{}_stored.wasm", CONTRACT_TRANSFER_TO_ACCOUNT_NAME),
            RuntimeArgs::default(),
        )
        .build();
        builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
        builder.exec_commit_finish(exec_request);
    }

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash = default_account
        .named_keys()
        .get(CONTRACT_TRANSFER_TO_ACCOUNT_NAME)
        .expect("contract_hash should exist")
        .into_hash()
        .expect("should be a hash");

    let response = builder
        .get_exec_response(0)
        .expect("there should be a response")
        .clone();
    let mut result = utils::get_success_result(&response);
    let gas = result.cost();
    let motes_alpha = Motes::from_gas(gas, DEFAULT_GAS_PRICE).expect("should have motes");

    let modified_balance_alpha: U512 = builder.get_purse_balance(default_account.main_purse());

    let transferred_amount: u64 = 1;
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // next make another deploy that USES stored payment logic
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_hash(
                contract_hash,
                TRANSFER_ENTRYPOINT,
                runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => transferred_amount },
            )
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => payment_purse_amount,
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([2; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec_commit_finish(exec_request);

    let modified_balance_bravo: U512 = builder.get_purse_balance(default_account.main_purse());

    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);

    let response = builder
        .get_exec_response(1)
        .expect("there should be a response")
        .clone();

    result = utils::get_success_result(&response);
    let gas = result.cost();
    let motes_bravo = Motes::from_gas(gas, DEFAULT_GAS_PRICE).expect("should have motes");

    let tally = motes_alpha.value()
        + motes_bravo.value()
        + U512::from(transferred_amount)
        + modified_balance_bravo;

    assert!(
        modified_balance_alpha < initial_balance,
        "balance should be less than initial balance"
    );

    assert!(
        modified_balance_bravo < modified_balance_alpha,
        "second modified balance should be less than first modified balance"
    );

    assert_eq!(
        initial_balance, tally,
        "no net resources should be gained or lost post-distribution"
    );
}
