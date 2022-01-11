use once_cell::sync::Lazy;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_RUN_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{
    core::engine_state::ExecuteRequest,
    shared::system_config::auction_costs::{
        DEFAULT_ADD_BID_COST, DEFAULT_DELEGATE_COST, DEFAULT_UNDELEGATE_COST,
        DEFAULT_WITHDRAW_BID_COST,
    },
};
use casper_types::{
    account::{Account, AccountHash},
    runtime_args,
    system::{
        auction::{self, DelegationRate},
        mint, standard_payment,
    },
    Gas, PublicKey, RuntimeArgs, SecretKey, U512,
};

const BOND_AMOUNT: u64 = 42;
const DELEGATE_AMOUNT: u64 = 100;
const DELEGATION_RATE: DelegationRate = 10;

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([99; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PUBLIC_KEY));

fn setup() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);
    let id: Option<u64> = None;
    let transfer_args_1 = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_1_ADDR,
        mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        mint::ARG_ID => id,
    };
    let transfer_request_1 =
        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args_1).build();
    builder.exec(transfer_request_1).expect_success().commit();
    builder
}

fn exec_and_assert_costs(
    builder: &mut InMemoryWasmTestBuilder,
    exec_request: ExecuteRequest,
    caller: Account,
    expected_tokens_paid: U512,
    expected_payment_charge: U512,
    expected_gas_cost: Gas,
) {
    let balance_before = builder.get_purse_balance(caller.main_purse());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request).expect_success().commit();

    let balance_after = builder.get_purse_balance(caller.main_purse());

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;
    assert_eq!(transaction_fee, expected_payment_charge);

    let expected = balance_before - expected_tokens_paid - transaction_fee;

    assert_eq!(
        balance_after,
        expected,
        "before and after should match; off by: {}",
        if expected > balance_after {
            expected - balance_after
        } else {
            balance_after - expected
        }
    );
    assert_eq!(builder.last_exec_gas_cost(), expected_gas_cost,);
}

#[ignore]
#[test]
fn should_not_charge_for_create_purse_in_first_time_bond() {
    let mut builder = setup();

    let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
    let account_1 = builder.get_account(*ACCOUNT_1_ADDR).unwrap();

    let bond_amount = U512::from(BOND_AMOUNT);
    // This amount should be enough to make first time add_bid call.
    let add_bid_payment_amount = U512::from(DEFAULT_ADD_BID_COST);

    let add_bid_request = {
        let sender = *DEFAULT_ACCOUNT_ADDR;
        let contract_hash = builder.get_auction_contract_hash();
        let entry_point = auction::METHOD_ADD_BID;
        let payment_args =
            runtime_args! { standard_payment::ARG_AMOUNT => add_bid_payment_amount, };
        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => bond_amount,
            auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash([43; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    exec_and_assert_costs(
        &mut builder,
        add_bid_request,
        default_account.clone(),
        bond_amount,
        add_bid_payment_amount,
        Gas::from(DEFAULT_ADD_BID_COST),
    );

    let delegate_payment_amount = U512::from(DEFAULT_DELEGATE_COST);
    let delegate_amount = U512::from(DELEGATE_AMOUNT);

    let delegate_request = {
        let sender = *ACCOUNT_1_ADDR;
        let contract_hash = builder.get_auction_contract_hash();
        let entry_point = auction::METHOD_DELEGATE;
        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => delegate_payment_amount,
        };
        let session_args = runtime_args! {
            auction::ARG_DELEGATOR => ACCOUNT_1_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => delegate_amount,
        };
        let deploy_hash = [55; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    exec_and_assert_costs(
        &mut builder,
        delegate_request,
        account_1.clone(),
        delegate_amount,
        delegate_payment_amount,
        Gas::from(DEFAULT_DELEGATE_COST),
    );

    let undelegate_payment_amount = U512::from(DEFAULT_UNDELEGATE_COST);
    let undelegate_amount = delegate_amount;

    let undelegate_request = {
        let sender = *ACCOUNT_1_ADDR;
        let contract_hash = builder.get_auction_contract_hash();
        let entry_point = auction::METHOD_UNDELEGATE;
        let payment_args = runtime_args! {
            standard_payment::ARG_AMOUNT => undelegate_payment_amount,
        };
        let session_args = runtime_args! {
            auction::ARG_DELEGATOR => ACCOUNT_1_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => undelegate_amount,
        };
        let deploy_hash = [56; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    exec_and_assert_costs(
        &mut builder,
        undelegate_request,
        account_1,
        U512::zero(), // we paid nothing in the deploy as we're unbonding
        undelegate_payment_amount,
        Gas::from(DEFAULT_UNDELEGATE_COST),
    );

    let unbond_amount = bond_amount;
    // This amount should be enough to make first time add_bid call.
    let withdraw_bid_payment_amount = U512::from(DEFAULT_WITHDRAW_BID_COST);

    let withdraw_bid_request = {
        let sender = *DEFAULT_ACCOUNT_ADDR;
        let contract_hash = builder.get_auction_contract_hash();
        let entry_point = auction::METHOD_WITHDRAW_BID;
        let payment_args =
            runtime_args! { standard_payment::ARG_AMOUNT => withdraw_bid_payment_amount, };
        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => unbond_amount,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash([58; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    exec_and_assert_costs(
        &mut builder,
        withdraw_bid_request,
        default_account,
        U512::zero(),
        withdraw_bid_payment_amount,
        Gas::from(DEFAULT_WITHDRAW_BID_COST),
    );
}
