use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_KEY,
        DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casper_types::{account::AccountHash, runtime_args, RuntimeArgs, U512};

const FUNCTION_NAME: &str = "transfer";
const CONTRACT_KEY_NAME: &str = "transfer_to_account";
const CONTRACT_TRANSFER_TO_ACCOUNT_NAME: &str = "transfer_to_account_u512";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const TRANSFER_AMOUNT: u64 = 1;
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

#[ignore]
#[test]
fn should_transfer_to_account_stored() {
    let mut builder = InMemoryWasmTestBuilder::default();

    // first, store transfer contract
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}_stored.wasm", CONTRACT_TRANSFER_TO_ACCOUNT_NAME),
        RuntimeArgs::default(),
    )
    .build();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let proposer_reward_starting_balance_alpha = builder.get_proposer_purse_balance();

    builder.exec_commit_finish(exec_request);

    let transaction_fee_alpha =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_alpha;

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash = default_account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract_hash should exist")
        .into_hash()
        .expect("should be a hash");

    let modified_balance_alpha: U512 = builder.get_purse_balance(default_account.main_purse());

    let transferred_amount: U512 = U512::from(TRANSFER_AMOUNT);
    let payment_purse_amount = *DEFAULT_PAYMENT;

    // next make another deploy that USES stored payment logic
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_session_hash(
                contract_hash.into(),
                FUNCTION_NAME,
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

    let proposer_reward_starting_balance_alpha = builder.get_proposer_purse_balance();

    builder.exec_commit_finish(exec_request);

    let transaction_fee_bravo =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_alpha;

    let modified_balance_bravo: U512 = builder.get_purse_balance(default_account.main_purse());

    let initial_balance: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);

    let tally =
        transaction_fee_alpha + transaction_fee_bravo + transferred_amount + modified_balance_bravo;

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
