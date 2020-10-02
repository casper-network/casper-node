use std::collections::BTreeMap;

use num_rational::Ratio;

use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{
    self,
    account::AccountHash,
    auction::{
        DelegationRate, ARG_AMOUNT, ARG_DELEGATION_RATE, ARG_DELEGATOR, ARG_DELEGATOR_PUBLIC_KEY,
        ARG_PUBLIC_KEY, ARG_REWARD_FACTORS, ARG_VALIDATOR, ARG_VALIDATOR_PUBLIC_KEY, BLOCK_REWARD,
        DELEGATION_RATE_DENOMINATOR, METHOD_DISTRIBUTE, METHOD_WITHDRAW_DELEGATOR_REWARD,
        METHOD_WITHDRAW_VALIDATOR_REWARD,
    },
    mint, runtime_args, PublicKey, RuntimeArgs, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);

const VALIDATOR_1: PublicKey = PublicKey::Ed25519([3; 32]);
const VALIDATOR_1_ADDR: AccountHash = AccountHash::new([4; 32]);
const VALIDATOR_2: PublicKey = PublicKey::Ed25519([5; 32]);
const VALIDATOR_2_ADDR: AccountHash = AccountHash::new([6; 32]);
const VALIDATOR_3: PublicKey = PublicKey::Ed25519([7; 32]);
const VALIDATOR_3_ADDR: AccountHash = AccountHash::new([8; 32]);
const DELEGATOR_1: PublicKey = PublicKey::Ed25519([204; 32]);
const DELEGATOR_1_ADDR: AccountHash = AccountHash::new([205; 32]);
const DELEGATOR_2: PublicKey = PublicKey::Ed25519([206; 32]);
const DELEGATOR_2_ADDR: AccountHash = AccountHash::new([207; 32]);
const DELEGATOR_3: PublicKey = PublicKey::Ed25519([208; 32]);
const DELEGATOR_3_ADDR: AccountHash = AccountHash::new([209; 32]);

fn withdraw_validator_reward(
    builder: &mut InMemoryWasmTestBuilder,
    sender: AccountHash,
    validator: PublicKey,
) -> U512 {
    const REWARD_PURSE: &str = "reward_purse"; // used in auction-bids contract

    let withdraw_request = ExecuteRequestBuilder::standard(
        sender,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_WITHDRAW_VALIDATOR_REWARD,
            ARG_VALIDATOR_PUBLIC_KEY => validator,
        },
    )
    .build();

    builder.exec(withdraw_request).commit().expect_success();

    let validator_reward_purse = builder
        .get_account(sender)
        .expect("should have account")
        .named_keys()
        .get(REWARD_PURSE)
        .expect("should have key")
        .into_uref()
        .expect("should be uref");

    builder.get_purse_balance(validator_reward_purse)
}

fn withdraw_delegator_reward(
    builder: &mut InMemoryWasmTestBuilder,
    sender: AccountHash,
    validator: PublicKey,
    delegator: PublicKey,
) -> U512 {
    const REWARD_PURSE: &str = "reward_purse"; // used in auction-bids contract

    let withdraw_request = ExecuteRequestBuilder::standard(
        sender,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_WITHDRAW_DELEGATOR_REWARD,
            ARG_VALIDATOR_PUBLIC_KEY => validator,
            ARG_DELEGATOR_PUBLIC_KEY => delegator,
        },
    )
    .build();

    builder.exec(withdraw_request).commit().expect_success();

    let validator_reward_purse = builder
        .get_account(sender)
        .expect("should have account")
        .named_keys()
        .get(REWARD_PURSE)
        .expect("should have key")
        .into_uref()
        .expect("should be uref");

    builder.get_purse_balance(validator_reward_purse)
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_zero() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let participant_portion = Ratio::new(U512::one(), U512::from(3));
    let remainders = Ratio::from(U512::from(2));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_2,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, BLOCK_REWARD);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance =
        (expected_total_reward * participant_portion + remainders).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let delegator_1_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_1, DELEGATOR_1);
    let expected_delegator_1_balance = (expected_total_reward * participant_portion).to_integer();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_2_ADDR, VALIDATOR_1, DELEGATOR_2);
    let expected_delegator_2_balance = (expected_total_reward * participant_portion).to_integer();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);

    // Subsequently, there should be no more rewards
    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    assert!(validator_1_balance.is_zero());

    let delegator_1_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_1, DELEGATOR_1);
    assert!(delegator_1_balance.is_zero());

    let delegator_2_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_2_ADDR, VALIDATOR_1, DELEGATOR_2);
    assert!(delegator_2_balance.is_zero());
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_half() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    // Validator share
    let validator_share = Ratio::new(U512::from(2), U512::from(3));
    let remainders = Ratio::from(U512::one());

    // Delegator shares
    let delegator_shares = Ratio::new(U512::one(), U512::from(6));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_2,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, BLOCK_REWARD);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance =
        (expected_total_reward * validator_share + remainders).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let delegator_1_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_1, DELEGATOR_1);
    let expected_delegator_1_balance = (expected_total_reward * delegator_shares).to_integer();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_2_ADDR, VALIDATOR_1, DELEGATOR_2);
    let expected_delegator_2_balance = (expected_total_reward * delegator_shares).to_integer();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_full() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_2,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, BLOCK_REWARD);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance =
        (expected_total_reward * Ratio::from(U512::one())).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let delegator_1_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_1, DELEGATOR_1);
    let expected_delegator_1_balance = U512::zero();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_2_ADDR, VALIDATOR_1, DELEGATOR_2);
    let expected_delegator_2_balance = U512::zero();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);
}

#[ignore]
#[test]
fn should_distribute_uneven_delegation_rate_zero() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 3_000_000;
    const DELEGATOR_2_STAKE: u64 = 4_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let validator_1_portion = Ratio::new(U512::one(), U512::from(8));
    let delegator_1_portion = Ratio::new(U512::from(3), U512::from(8));
    let delegator_2_portion = Ratio::new(U512::from(4), U512::from(8));

    let remainder = Ratio::from(U512::from(1));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_2,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, BLOCK_REWARD);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance =
        (expected_total_reward * validator_1_portion + remainder).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let delegator_1_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_1, DELEGATOR_1);
    let expected_delegator_1_balance = (expected_total_reward * delegator_1_portion).to_integer();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_2_ADDR, VALIDATOR_1, DELEGATOR_2);
    let expected_delegator_2_balance = (expected_total_reward * delegator_2_portion).to_integer();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);
}

#[ignore]
#[test]
fn should_distribute_by_factor() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const VALIDATOR_2_STAKE: u64 = 1_000_000;
    const VALIDATOR_3_STAKE: u64 = 1_000_000;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    const VALIDATOR_1_REWARD_FACTOR: u64 = 333333333334;
    const VALIDATOR_2_REWARD_FACTOR: u64 = 333333333333;
    const VALIDATOR_3_REWARD_FACTOR: u64 = 333333333333;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let one_third = Ratio::new(U512::one(), U512::from(3));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_3_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        validator_3_add_bid_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_2_ADDR, VALIDATOR_2);
    let expected_validator_2_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_3_ADDR, VALIDATOR_3);
    let expected_validator_3_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let total_payout = validator_1_balance + validator_2_balance + validator_3_balance;
    let rounded_amount = U512::from(2);
    assert_eq!(total_payout, expected_total_reward_integer - rounded_amount);
}

#[ignore]
#[test]
fn should_distribute_by_factor_regardless_of_stake() {
    const VALIDATOR_1_STAKE: u64 = 4_000_000;
    const VALIDATOR_2_STAKE: u64 = 2_000_000;
    const VALIDATOR_3_STAKE: u64 = 1_000_000;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    const VALIDATOR_1_REWARD_FACTOR: u64 = 333333333334;
    const VALIDATOR_2_REWARD_FACTOR: u64 = 333333333333;
    const VALIDATOR_3_REWARD_FACTOR: u64 = 333333333333;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let one_third = Ratio::new(U512::one(), U512::from(3));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_3_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        validator_3_add_bid_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_2_ADDR, VALIDATOR_2);
    let expected_validator_2_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_3_ADDR, VALIDATOR_3);
    let expected_validator_3_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let total_payout = validator_1_balance + validator_2_balance + validator_3_balance;
    let rounded_amount = U512::from(2);
    assert_eq!(total_payout, expected_total_reward_integer - rounded_amount);
}

#[ignore]
#[test]
fn should_distribute_by_factor_uneven() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const VALIDATOR_2_STAKE: u64 = 1_000_000;
    const VALIDATOR_3_STAKE: u64 = 1_000_000;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    const VALIDATOR_1_REWARD_FACTOR: u64 = 500000000000;
    const VALIDATOR_2_REWARD_FACTOR: u64 = 300000000000;
    const VALIDATOR_3_REWARD_FACTOR: u64 = 200000000000;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let one_half = Ratio::new(U512::one(), U512::from(2));
    let three_tenths = Ratio::new(U512::from(3), U512::from(10));
    let one_fifth = Ratio::new(U512::from(1), U512::from(5));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_3_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        validator_3_add_bid_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance = (expected_total_reward * one_half).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_2_ADDR, VALIDATOR_2);
    let expected_validator_2_balance = (expected_total_reward * three_tenths).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_3_ADDR, VALIDATOR_3);
    let expected_validator_3_balance = (expected_total_reward * one_fifth).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let total_payout = validator_1_balance + validator_2_balance + validator_3_balance;
    let rounded_amount = U512::from(1);
    assert_eq!(total_payout, expected_total_reward_integer - rounded_amount);
}

#[ignore]
#[test]
fn should_distribute_with_multiple_validators_and_delegators() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const VALIDATOR_2_STAKE: u64 = 1_000_000;
    const VALIDATOR_3_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;
    const VALIDATOR_2_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 4;
    const VALIDATOR_3_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    const VALIDATOR_1_REWARD_FACTOR: u64 = 200000000000;
    const VALIDATOR_2_REWARD_FACTOR: u64 = 300000000000;
    const VALIDATOR_3_REWARD_FACTOR: u64 = 500000000000;

    const DELEGATOR_1_STAKE: u64 = 3_000_000;
    const DELEGATOR_2_STAKE: u64 = 4_000_000;
    const DELEGATOR_3_STAKE: u64 = 1_000_000;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let remainder = U512::one();

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_2_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_3_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_2,
        },
    )
    .build();

    let delegator_3_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_3_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_3_STAKE),
            ARG_VALIDATOR => VALIDATOR_2,
            ARG_DELEGATOR => DELEGATOR_3,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_3_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        delegator_3_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        validator_3_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
        delegator_3_delegate_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_2_ADDR, VALIDATOR_2);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_3_ADDR, VALIDATOR_3);

    let delegator_1_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_1, DELEGATOR_1);

    let delegator_2_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_2_ADDR, VALIDATOR_1, DELEGATOR_2);

    let delegator_3_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_3_ADDR, VALIDATOR_2, DELEGATOR_3);

    let total_payout: U512 = [
        validator_1_balance,
        validator_2_balance,
        validator_3_balance,
        delegator_1_balance,
        delegator_2_balance,
        delegator_3_balance,
    ]
    .iter()
    .cloned()
    .sum();

    assert_eq!(total_payout, expected_total_reward_integer - remainder);
}

#[ignore]
#[test]
fn should_distribute_with_multiple_validators_and_shared_delegator() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const VALIDATOR_2_STAKE: u64 = 1_000_000;
    const VALIDATOR_3_STAKE: u64 = 1_000_000;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    const VALIDATOR_1_REWARD_FACTOR: u64 = 333333333334;
    const VALIDATOR_2_REWARD_FACTOR: u64 = 333333333333;
    const VALIDATOR_3_REWARD_FACTOR: u64 = 333333333333;

    const DELEGATOR_1_STAKE: u64 = 1_000_000;

    let expected_total_reward = mint::round_seigniorage_rate() * mint::initial_supply_motes();
    let expected_total_reward_integer = expected_total_reward.to_integer();

    let validator_1_portion = Ratio::new(U512::from(1), U512::from(4));
    let validator_2_portion = Ratio::new(U512::from(1), U512::from(4));
    let validator_3_portion = Ratio::new(U512::from(1), U512::from(4));
    let delegator_1_validator_1_portion = Ratio::new(U512::from(1), U512::from(12));
    let delegator_1_validator_2_portion = Ratio::new(U512::from(1), U512::from(12));
    let delegator_1_validator_3_portion = Ratio::new(U512::from(1), U512::from(12));

    let remainder = U512::from(2);

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => DELEGATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let delegator_1_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_2,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let delegator_1_validator_3_delegate_request = ExecuteRequestBuilder::standard(
        DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_3,
            ARG_DELEGATOR => DELEGATOR_1,
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_3_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        delegator_3_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        validator_3_add_bid_request,
        delegator_1_validator_1_delegate_request,
        delegator_1_validator_2_delegate_request,
        delegator_1_validator_3_delegate_request,
    ];

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_1_ADDR, VALIDATOR_1);
    let expected_validator_1_balance = (expected_total_reward * validator_1_portion).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_2_ADDR, VALIDATOR_2);
    let expected_validator_2_balance = (expected_total_reward * validator_2_portion).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, VALIDATOR_3_ADDR, VALIDATOR_3);
    let expected_validator_3_balance = (expected_total_reward * validator_3_portion).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let delegator_1_validator_1_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_1, DELEGATOR_1);
    let expected_delegator_1_validator_1_balance =
        (expected_total_reward * delegator_1_validator_1_portion).to_integer();
    assert_eq!(
        delegator_1_validator_1_balance,
        expected_delegator_1_validator_1_balance
    );

    let delegator_1_validator_2_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_2, DELEGATOR_1);
    let expected_delegator_1_validator_2_balance =
        (expected_total_reward * delegator_1_validator_2_portion).to_integer();
    assert_eq!(
        delegator_1_validator_2_balance,
        expected_delegator_1_validator_2_balance
    );

    let delegator_1_validator_3_balance =
        withdraw_delegator_reward(&mut builder, DELEGATOR_1_ADDR, VALIDATOR_3, DELEGATOR_1);
    let expected_delegator_1_validator_3_balance =
        (expected_total_reward * delegator_1_validator_3_portion).to_integer();
    assert_eq!(
        delegator_1_validator_3_balance,
        expected_delegator_1_validator_3_balance
    );

    let total_payout: U512 = [
        validator_1_balance,
        validator_2_balance,
        validator_3_balance,
        delegator_1_validator_1_balance,
        delegator_1_validator_2_balance,
        delegator_1_validator_3_balance,
    ]
    .iter()
    .cloned()
    .sum();

    assert_eq!(total_payout, expected_total_reward_integer - remainder);
}
