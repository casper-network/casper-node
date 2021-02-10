use std::collections::BTreeMap;

use num_rational::Ratio;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, UpgradeRequestBuilder,
        DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_RUN_GENESIS_REQUEST,
        TIMESTAMP_MILLIS_INCREMENT,
    },
    DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::{
    self,
    account::AccountHash,
    auction::{
        DelegationRate, SeigniorageAllocation, ARG_AMOUNT, ARG_DELEGATION_RATE, ARG_DELEGATOR,
        ARG_DELEGATOR_PUBLIC_KEY, ARG_PUBLIC_KEY, ARG_REWARD_FACTORS, ARG_VALIDATOR,
        ARG_VALIDATOR_PUBLIC_KEY, BLOCK_REWARD, DELEGATION_RATE_DENOMINATOR, METHOD_DISTRIBUTE,
        METHOD_WITHDRAW_DELEGATOR_REWARD, METHOD_WITHDRAW_VALIDATOR_REWARD,
    },
    runtime_args, Key, ProtocolVersion, PublicKey, RuntimeArgs, SecretKey, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_TARGET: &str = "target";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);

static VALIDATOR_1: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([3; SecretKey::ED25519_LENGTH]).into());
static VALIDATOR_2: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([5; SecretKey::ED25519_LENGTH]).into());
static VALIDATOR_3: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([7; SecretKey::ED25519_LENGTH]).into());
static DELEGATOR_1: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([204; SecretKey::ED25519_LENGTH]).into());
static DELEGATOR_2: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([206; SecretKey::ED25519_LENGTH]).into());
static DELEGATOR_3: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([208; SecretKey::ED25519_LENGTH]).into());

static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_1));
static VALIDATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_2));
static VALIDATOR_3_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_3));
static DELEGATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_1));
static DELEGATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_2));
static DELEGATOR_3_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_3));
static GENESIS_ROUND_SEIGNIORAGE_RATE: Lazy<Ratio<U512>> = Lazy::new(|| {
    Ratio::new(
        U512::from(*DEFAULT_ROUND_SEIGNIORAGE_RATE.numer()),
        U512::from(*DEFAULT_ROUND_SEIGNIORAGE_RATE.denom()),
    )
});

fn withdraw_validator_reward(
    builder: &mut InMemoryWasmTestBuilder,
    sender: AccountHash,
    validator: PublicKey,
    protocol_version: Option<ProtocolVersion>,
) -> U512 {
    let withdraw_request = {
        let mut builder = ExecuteRequestBuilder::standard(
            sender,
            CONTRACT_AUCTION_BIDS,
            runtime_args! {
                ARG_ENTRY_POINT => METHOD_WITHDRAW_VALIDATOR_REWARD,
                ARG_VALIDATOR_PUBLIC_KEY => validator,
            },
        );

        if let Some(protocol_version) = protocol_version {
            builder = builder.with_protocol_version(protocol_version)
        }

        builder.build()
    };

    let account = builder.get_account(sender).expect("should have account");

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(withdraw_request).commit().expect_success();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    balance_after + transaction_fee - balance_before
}

fn withdraw_delegator_reward(
    builder: &mut InMemoryWasmTestBuilder,
    sender: AccountHash,
    validator: PublicKey,
    delegator: PublicKey,
    protocol_version: Option<ProtocolVersion>,
) -> U512 {
    let withdraw_request = {
        let mut builder = ExecuteRequestBuilder::standard(
            sender,
            CONTRACT_AUCTION_BIDS,
            runtime_args! {
                ARG_ENTRY_POINT => METHOD_WITHDRAW_DELEGATOR_REWARD,
                ARG_VALIDATOR_PUBLIC_KEY => validator,
                ARG_DELEGATOR_PUBLIC_KEY => delegator,
            },
        );
        if let Some(protocol_version) = protocol_version {
            builder = builder.with_protocol_version(protocol_version);
        }
        builder.build()
    };

    let account = builder.get_account(sender).expect("should have account");

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(withdraw_request).commit().expect_success();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    balance_after + transaction_fee - balance_before
}
#[ignore]
#[test]
fn should_distribute_delegation_rate_zero() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let participant_portion = Ratio::new(U512::one(), U512::from(3));
    let remainders = Ratio::from(U512::zero());

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);

    let expected_validator_1_balance_ratio =
        expected_total_reward * participant_portion + remainders;
    let expected_validator_1_balance = expected_validator_1_balance_ratio.to_integer();
    assert_eq!(
        validator_1_balance, expected_validator_1_balance,
        "rhs {}",
        expected_validator_1_balance_ratio
    );

    let delegator_1_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_balance = (expected_total_reward * participant_portion).to_integer();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        None,
    );
    let expected_delegator_2_balance = (expected_total_reward * participant_portion).to_integer();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);

    // Subsequently, there should be no more rewards
    let validator_1_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    assert!(validator_1_balance.is_zero());

    let delegator_1_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
    assert!(delegator_1_balance.is_zero());

    let delegator_2_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        None,
    );
    assert!(delegator_2_balance.is_zero());

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_1).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == expected_delegator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_2).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == expected_delegator_1_balance
    ));
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_half() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    // Validator share
    let validator_share = Ratio::new(U512::from(2), U512::from(3));
    let remainders = Ratio::from(U512::from(2));
    let rounded_amount = U512::from(2);

    // Delegator shares
    let delegator_shares = Ratio::new(U512::one(), U512::from(6));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance =
        (expected_total_reward * validator_share + remainders - rounded_amount).to_integer();

    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let delegator_1_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_balance = (expected_total_reward * delegator_shares).to_integer();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        None,
    );
    let expected_delegator_2_balance = (expected_total_reward * delegator_shares).to_integer();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_1).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == expected_delegator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_2).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == expected_delegator_1_balance
    ));
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_full() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance =
        (expected_total_reward * Ratio::from(U512::one())).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let delegator_1_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_balance = U512::zero();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        None,
    );
    let expected_delegator_2_balance = U512::zero();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_1).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == expected_delegator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_2).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == expected_delegator_1_balance
    ));
}

#[ignore]
#[test]
fn should_distribute_uneven_delegation_rate_zero() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 3_000_000;
    const DELEGATOR_2_STAKE: u64 = 4_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let validator_1_portion = Ratio::new(U512::one(), U512::from(8));
    let delegator_1_portion = Ratio::new(U512::from(3), U512::from(8));
    let delegator_2_portion = Ratio::new(U512::from(4), U512::from(8));

    let remainder = Ratio::from(U512::from(1));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance =
        (expected_total_reward * validator_1_portion + remainder).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let delegator_1_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_balance = (expected_total_reward * delegator_1_portion).to_integer();
    assert_eq!(delegator_1_balance, expected_delegator_1_balance);

    let delegator_2_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        None,
    );
    let expected_delegator_2_balance = (expected_total_reward * delegator_2_portion).to_integer();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_balance + delegator_1_balance + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_1).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == expected_delegator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_2).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == expected_delegator_2_balance
    ));
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

    let one_third = Ratio::new(U512::one(), U512::from(3));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_3,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let rounded_amount = U512::one();

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_2_ADDR, *VALIDATOR_2, None);
    let expected_validator_2_balance =
        (expected_total_reward * one_third - rounded_amount).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_3_ADDR, *VALIDATOR_3, None);

    let expected_validator_3_balance =
        (expected_total_reward * one_third - rounded_amount).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let total_payout = validator_1_balance + validator_2_balance + validator_3_balance;
    let rounded_amount = U512::from(2);
    assert_eq!(total_payout, expected_total_reward_integer - rounded_amount);

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_2).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_2 && *amount == expected_validator_2_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_3).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_3 && *amount == expected_validator_3_balance
    ));
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

    let one_third = Ratio::new(U512::one(), U512::from(3));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_3,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance = (expected_total_reward * one_third).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let rounded_amount = U512::one();

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_2_ADDR, *VALIDATOR_2, None);
    let expected_validator_2_balance =
        (expected_total_reward * one_third - rounded_amount).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_3_ADDR, *VALIDATOR_3, None);
    let expected_validator_3_balance =
        (expected_total_reward * one_third - rounded_amount).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let total_payout = validator_1_balance + validator_2_balance + validator_3_balance;
    let rounded_amount = U512::from(2);
    assert_eq!(total_payout, expected_total_reward_integer - rounded_amount);

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_2).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_2 && *amount == expected_validator_2_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_3).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_3 && *amount == expected_validator_3_balance
    ));
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

    let one_half = Ratio::new(U512::one(), U512::from(2));
    let three_tenths = Ratio::new(U512::from(3), U512::from(10));
    let one_fifth = Ratio::new(U512::from(1), U512::from(5));

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_3,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance = (expected_total_reward * one_half).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_2_ADDR, *VALIDATOR_2, None);
    let expected_validator_2_balance = (expected_total_reward * three_tenths).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_3_ADDR, *VALIDATOR_3, None);
    let expected_validator_3_balance = (expected_total_reward * one_fifth).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let total_payout = validator_1_balance + validator_2_balance + validator_3_balance;
    let rounded_amount = U512::one();
    assert_eq!(total_payout, expected_total_reward_integer - rounded_amount);

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_2).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_2 && *amount == expected_validator_2_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_3).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_3 && *amount == expected_validator_3_balance
    ));
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

    let remainder = U512::one();

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_2_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_3_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_3,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
        },
    )
    .build();

    let delegator_3_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_3_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_3_STAKE),
            ARG_VALIDATOR => *VALIDATOR_2,
            ARG_DELEGATOR => *DELEGATOR_3,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_2_ADDR, *VALIDATOR_2, None);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_3_ADDR, *VALIDATOR_3, None);

    let delegator_1_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );

    let delegator_2_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        None,
    );

    let delegator_3_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_3_ADDR,
        *VALIDATOR_2,
        *DELEGATOR_3,
        None,
    );

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

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_2).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_2 && *amount == validator_2_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_3).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_3 && *amount == validator_3_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_1).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_2).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_2_balance
    ));

    assert!(matches!(
        era_info.select(*DELEGATOR_3).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_3 && *amount == delegator_3_balance
    ));
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
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_3,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_1_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_2,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_1_validator_3_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_3,
            ARG_DELEGATOR => *DELEGATOR_1,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
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
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance = (expected_total_reward * validator_1_portion).to_integer();
    assert_eq!(validator_1_balance, expected_validator_1_balance);

    let validator_2_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_2_ADDR, *VALIDATOR_2, None);
    let expected_validator_2_balance = (expected_total_reward * validator_2_portion).to_integer();
    assert_eq!(validator_2_balance, expected_validator_2_balance);

    let validator_3_balance =
        withdraw_validator_reward(&mut builder, *VALIDATOR_3_ADDR, *VALIDATOR_3, None);
    let expected_validator_3_balance = (expected_total_reward * validator_3_portion).to_integer();
    assert_eq!(validator_3_balance, expected_validator_3_balance);

    let delegator_1_validator_1_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_validator_1_balance =
        (expected_total_reward * delegator_1_validator_1_portion).to_integer();
    assert_eq!(
        delegator_1_validator_1_balance,
        expected_delegator_1_validator_1_balance
    );

    let rounded_amount = U512::one();
    let delegator_1_validator_2_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_2,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_validator_2_balance =
        (expected_total_reward * delegator_1_validator_2_portion - rounded_amount).to_integer();
    assert_eq!(
        delegator_1_validator_2_balance,
        expected_delegator_1_validator_2_balance
    );

    let delegator_1_validator_3_balance = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_3,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_validator_3_balance =
        (expected_total_reward * delegator_1_validator_3_portion - rounded_amount).to_integer();
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

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    assert!(matches!(
        era_info.select(*VALIDATOR_1).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_2).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_2 && *amount == expected_validator_2_balance
    ));

    assert!(matches!(
        era_info.select(*VALIDATOR_3).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_3 && *amount == expected_validator_3_balance
    ));

    let delegator_1_allocations: Vec<SeigniorageAllocation> =
        era_info.select(*DELEGATOR_1).cloned().collect();

    assert_eq!(delegator_1_allocations.len(), 3);

    assert!(
        delegator_1_allocations.contains(&SeigniorageAllocation::delegator(
            *DELEGATOR_1,
            *VALIDATOR_1,
            expected_delegator_1_validator_1_balance,
        ))
    );

    assert!(
        delegator_1_allocations.contains(&SeigniorageAllocation::delegator(
            *DELEGATOR_1,
            *VALIDATOR_2,
            expected_delegator_1_validator_2_balance,
        ))
    );

    assert!(
        delegator_1_allocations.contains(&SeigniorageAllocation::delegator(
            *DELEGATOR_1,
            *VALIDATOR_3,
            expected_delegator_1_validator_3_balance,
        ))
    );
}

#[ignore]
#[should_panic]
#[test]
fn should_prevent_theft_of_validator_reward() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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

    withdraw_validator_reward(&mut builder, *DELEGATOR_1_ADDR, *VALIDATOR_1, None);
}

#[ignore]
#[should_panic]
#[test]
fn should_prevent_theft_of_delegator_reward() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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

    withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
}

#[ignore]
#[test]
fn should_increase_total_supply_after_distribute() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const VALIDATOR_2_STAKE: u64 = 1_000_000;
    const VALIDATOR_3_STAKE: u64 = 1_000_000;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    const VALIDATOR_1_REWARD_FACTOR: u64 = 333333333334;
    const VALIDATOR_2_REWARD_FACTOR: u64 = 333333333333;
    const VALIDATOR_3_REWARD_FACTOR: u64 = 333333333333;

    const DELEGATOR_1_STAKE: u64 = 1_000_000;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_3_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_2,
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_3,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_1_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_2,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_1_validator_3_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_3,
            ARG_DELEGATOR => *DELEGATOR_1,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    let post_genesis_supply = builder.total_supply(None);

    assert_eq!(
        initial_supply, post_genesis_supply,
        "total supply should remain unchanged prior to first distribution"
    );

    // run auction
    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let post_auction_supply = builder.total_supply(None);
    assert_eq!(
        initial_supply, post_auction_supply,
        "total supply should remain unchanged regardless of auction"
    );

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, VALIDATOR_1_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_2, VALIDATOR_2_REWARD_FACTOR);
        tmp.insert(*VALIDATOR_3, VALIDATOR_3_REWARD_FACTOR);
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

    let post_distribute_supply = builder.total_supply(None);
    assert!(
        initial_supply < post_distribute_supply,
        "total supply should increase after distribute"
    );
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_full_after_upgrading() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => *VALIDATOR_1,
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_1,
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => *VALIDATOR_1,
            ARG_DELEGATOR => *DELEGATOR_2,
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

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    // initial token supply
    let initial_supply = builder.total_supply(None);
    let expected_total_reward_before = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward_before.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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

    let validator_1_balance_before =
        withdraw_validator_reward(&mut builder, *VALIDATOR_1_ADDR, *VALIDATOR_1, None);
    let expected_validator_1_balance_before =
        (expected_total_reward_before * Ratio::from(U512::one())).to_integer();
    assert_eq!(
        validator_1_balance_before,
        expected_validator_1_balance_before
    );

    let delegator_1_balance_before = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        None,
    );
    let expected_delegator_1_balance_before = U512::zero();
    assert_eq!(
        delegator_1_balance_before,
        expected_delegator_1_balance_before
    );

    let delegator_2_balance_before = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        None,
    );
    let expected_delegator_2_balance = U512::zero();
    assert_eq!(delegator_2_balance_before, expected_delegator_2_balance);

    let total_payout_before =
        validator_1_balance_before + delegator_1_balance_before + delegator_2_balance_before;
    assert_eq!(total_payout_before, expected_total_reward_integer);

    //
    // Update round seigniorage rate into 50% of default value
    //
    let new_seigniorage_multiplier = Ratio::new_raw(1, 10);
    let new_round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE * new_seigniorage_multiplier;

    let old_protocol_version = *DEFAULT_PROTOCOL_VERSION;
    let sem_ver = old_protocol_version.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let mut upgrade_request = {
        const DEFAULT_ACTIVATION_POINT: u64 = 1;
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(old_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_round_seigniorage_rate(new_round_seigniorage_rate)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let initial_supply = builder.total_supply(None);

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*VALIDATOR_1, BLOCK_REWARD);
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
    .with_protocol_version(new_protocol_version)
    .build();

    let new_round_seigniorage_rate = {
        let (numer, denom) = new_round_seigniorage_rate.into();
        Ratio::new(numer.into(), denom.into())
    };

    builder.exec(distribute_request).commit().expect_success();

    let expected_total_reward_after = new_round_seigniorage_rate * initial_supply;

    let validator_1_balance_after = withdraw_validator_reward(
        &mut builder,
        *VALIDATOR_1_ADDR,
        *VALIDATOR_1,
        Some(new_protocol_version),
    );
    let expected_validator_1_balance_after =
        (expected_total_reward_after * Ratio::from(U512::one())).to_integer();
    assert_eq!(
        validator_1_balance_after,
        expected_validator_1_balance_after
    );

    let delegator_1_balance_after = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_1_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_1,
        Some(new_protocol_version),
    );
    let expected_delegator_1_balance_after = U512::zero();
    assert_eq!(
        delegator_1_balance_after,
        expected_delegator_1_balance_after
    );

    let delegator_2_balance_after = withdraw_delegator_reward(
        &mut builder,
        *DELEGATOR_2_ADDR,
        *VALIDATOR_1,
        *DELEGATOR_2,
        Some(new_protocol_version),
    );
    let expected_delegator_2_balance_after = U512::zero();
    assert_eq!(
        delegator_2_balance_after,
        expected_delegator_2_balance_after
    );

    let expected_total_reward_after = expected_total_reward_after.to_integer();

    let total_payout_after =
        validator_1_balance_after + delegator_1_balance_after + delegator_2_balance_after;
    assert_eq!(total_payout_after, expected_total_reward_after);

    assert!(expected_validator_1_balance_before > expected_validator_1_balance_after); // expected amount after decreasing seigniorage rate is lower than the first amount
    assert!(total_payout_before > total_payout_after); // expected total payout after decreasing
                                                       // rate is lower than the first payout
}
