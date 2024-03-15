use num_rational::Ratio;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, TransferRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY, LOCAL_GENESIS_REQUEST,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::engine_state::{
    engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT, ExecuteRequest,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{
        auction::{self, DelegationRate},
        standard_payment,
    },
    AddressableEntity, FeeHandling, Gas, PublicKey, RefundHandling, SecretKey, U512,
};

const BOND_AMOUNT: u64 = 42;
const DELEGATE_AMOUNT: u64 = 100 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATION_RATE: DelegationRate = 10;

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([99; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PUBLIC_KEY));

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();

    let chainspec = builder
        .chainspec()
        .clone()
        .with_refund_handling(RefundHandling::Refund {
            refund_ratio: Ratio::new(1, 1),
        })
        .with_fee_handling(FeeHandling::PayToProposer);
    builder.with_chainspec(chainspec);

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let transfer_request =
        TransferRequestBuilder::new(MINIMUM_ACCOUNT_CREATION_BALANCE, *ACCOUNT_1_ADDR).build();
    builder
        .transfer_and_commit(transfer_request)
        .expect_success();
    builder
}

fn exec_and_assert_costs(
    builder: &mut LmdbWasmTestBuilder,
    exec_request: ExecuteRequest,
    caller: AddressableEntity,
    expected_tokens_paid: U512,
    _payment_amount: U512,
    expected_gas_cost: Gas,
) {
    let balance_before = builder.get_purse_balance(caller.main_purse());

    let proposer_balance_before = builder.get_proposer_purse_balance();

    builder.exec(exec_request).expect_success().commit();

    let balance_after = builder.get_purse_balance(caller.main_purse());

    let proposer_fee = builder.get_proposer_purse_balance() - proposer_balance_before;

    assert_eq!(
        proposer_fee,
        expected_gas_cost.value(),
        "with PayToProposer && 100% refund of unspent, the fee should equal the gas cost"
    );

    let expected = balance_before - expected_tokens_paid - proposer_fee;

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

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .unwrap();
    let account_1 = builder.get_entity_by_account_hash(*ACCOUNT_1_ADDR).unwrap();

    let bond_amount = U512::from(BOND_AMOUNT);
    // This amount should be enough to make first time add_bid call.
    let add_bid_cost = builder.get_auction_costs().add_bid;

    let pay_cost = builder
        .chainspec()
        .system_costs_config
        .standard_payment_costs()
        .pay;

    let add_bid_payment_amount = U512::from(add_bid_cost + pay_cost) * 2;

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

        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash([43; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    exec_and_assert_costs(
        &mut builder,
        add_bid_request,
        default_account.clone(),
        bond_amount,
        add_bid_payment_amount,
        Gas::from(add_bid_cost),
    );

    let delegate_cost = builder.get_auction_costs().delegate;
    let delegate_payment_amount = U512::from(delegate_cost);
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

        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    exec_and_assert_costs(
        &mut builder,
        delegate_request,
        account_1.clone(),
        delegate_amount,
        delegate_payment_amount,
        Gas::from(delegate_cost),
    );

    let undelegate_cost = builder.get_auction_costs().undelegate;
    let undelegate_payment_amount = U512::from(undelegate_cost);
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

        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    exec_and_assert_costs(
        &mut builder,
        undelegate_request,
        account_1,
        U512::zero(), // we paid nothing in the deploy as we're unbonding
        undelegate_payment_amount,
        Gas::from(undelegate_cost),
    );

    let unbond_amount = bond_amount;
    // This amount should be enough to make first time add_bid call.
    let withdraw_bid_cost = builder.get_auction_costs().withdraw_bid;
    let withdraw_bid_payment_amount = U512::from(withdraw_bid_cost);

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

        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash([58; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    exec_and_assert_costs(
        &mut builder,
        withdraw_bid_request,
        default_account,
        U512::zero(),
        withdraw_bid_payment_amount,
        Gas::from(withdraw_bid_cost),
    );
}
