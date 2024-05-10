use std::collections::BTreeMap;

use num_rational::Ratio;
use num_traits::{CheckedMul, CheckedSub};
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_MINIMUM_DELEGATION_AMOUNT, DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE,
    LOCAL_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_ROUND_SEIGNIORAGE_RATE,
    SYSTEM_ADDR, TIMESTAMP_MILLIS_INCREMENT,
};
use casper_storage::data_access_layer::AuctionMethod;
use casper_types::{
    self,
    account::AccountHash,
    runtime_args,
    system::auction::{
        self, BidsExt as _, DelegationRate, Delegator, EraInfo, SeigniorageAllocation,
        SeigniorageRecipientsSnapshot, ValidatorBid, ARG_AMOUNT, ARG_DELEGATION_RATE,
        ARG_DELEGATOR, ARG_PUBLIC_KEY, ARG_REWARDS_MAP, ARG_VALIDATOR, DELEGATION_RATE_DENOMINATOR,
        METHOD_DISTRIBUTE, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    },
    EntityAddr, EraId, Key, ProtocolVersion, PublicKey, SecretKey, Timestamp, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_TARGET: &str = "target";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([5; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_3: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([7; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([204; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([206; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_3: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([208; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_1));
static VALIDATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_2));
static VALIDATOR_3_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_3));
static DELEGATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_1));
static DELEGATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_2));
static DELEGATOR_3_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_3));
static GENESIS_ROUND_SEIGNIORAGE_RATE: Lazy<Ratio<U512>> = Lazy::new(|| {
    Ratio::new(
        U512::from(*PRODUCTION_ROUND_SEIGNIORAGE_RATE.numer()),
        U512::from(*PRODUCTION_ROUND_SEIGNIORAGE_RATE.denom()),
    )
});

fn get_validator_bid(
    builder: &mut LmdbWasmTestBuilder,
    validator_public_key: PublicKey,
) -> Option<ValidatorBid> {
    let bids = builder.get_bids();
    bids.validator_bid(&validator_public_key)
}

fn get_delegator_bid(
    builder: &mut LmdbWasmTestBuilder,
    validator: PublicKey,
    delegator: PublicKey,
) -> Option<Delegator> {
    let bids = builder.get_bids();
    bids.delegator_by_public_keys(&validator, &delegator)
}

fn withdraw_bid(
    builder: &mut LmdbWasmTestBuilder,
    sender: AccountHash,
    validator: PublicKey,
    amount: U512,
) {
    let auction = builder.get_auction_contract_hash();
    let withdraw_bid_args = runtime_args! {
        ARG_PUBLIC_KEY => validator,
        ARG_AMOUNT => amount,
    };
    let withdraw_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        sender,
        auction,
        auction::METHOD_WITHDRAW_BID,
        withdraw_bid_args,
    )
    .build();
    builder.exec(withdraw_bid_request).expect_success().commit();
}

fn undelegate(
    builder: &mut LmdbWasmTestBuilder,
    sender: AccountHash,
    delegator: PublicKey,
    validator: PublicKey,
    amount: U512,
) {
    let auction = builder.get_auction_contract_hash();
    let undelegate_args = runtime_args! {
        ARG_DELEGATOR => delegator,
        ARG_VALIDATOR => validator,
        ARG_AMOUNT => amount,
    };
    let undelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        sender,
        auction,
        auction::METHOD_UNDELEGATE,
        undelegate_args,
    )
    .build();
    builder.exec(undelegate_request).expect_success().commit();
}

fn get_delegator_staked_amount(
    builder: &mut LmdbWasmTestBuilder,
    validator_public_key: PublicKey,
    delegator_public_key: PublicKey,
) -> U512 {
    let bids = builder.get_bids();
    let delegator = bids
        .delegator_by_public_keys(&validator_public_key, &delegator_public_key)
        .expect("bid should exist for validator-{validator_public_key}, delegator-{delegator_public_key}");

    delegator.staked_amount()
}

fn get_era_info(builder: &mut LmdbWasmTestBuilder) -> EraInfo {
    let era_info_value = builder
        .query(None, Key::EraSummary, &[])
        .expect("should have value");

    era_info_value
        .as_era_info()
        .cloned()
        .expect("should be era info")
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_zero() {
    const VALIDATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const DELEGATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const DELEGATOR_2_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const TOTAL_DELEGATOR_STAKE: u64 = DELEGATOR_1_STAKE + DELEGATOR_2_STAKE;
    const TOTAL_STAKE: u64 = VALIDATOR_1_STAKE + TOTAL_DELEGATOR_STAKE;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let total_payout = builder.base_round_reward(None, protocol_version);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();
    assert_eq!(total_payout, expected_total_reward_integer);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();
        assert!(
            builder.step(step_request).is_success(),
            "must execute step successfully"
        );
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let delegators_share = {
        let commission_rate = Ratio::new(
            U512::from(VALIDATOR_1_DELEGATION_RATE),
            U512::from(DELEGATION_RATE_DENOMINATOR),
        );
        let reward_multiplier =
            Ratio::new(U512::from(TOTAL_DELEGATOR_STAKE), U512::from(TOTAL_STAKE));
        let delegator_reward = expected_total_reward
            .checked_mul(&reward_multiplier)
            .expect("must get delegator reward");
        let commission = delegator_reward
            .checked_mul(&commission_rate)
            .expect("must get commission");
        delegator_reward.checked_sub(&commission).unwrap()
    };

    let delegator_1_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_1_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        delegators_share
            .checked_mul(&reward_multiplier)
            .map(|ratio| ratio.to_integer())
            .expect("must get delegator 1 payout")
    };

    let delegator_2_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_2_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        delegators_share
            .checked_mul(&reward_multiplier)
            .map(|ratio| ratio.to_integer())
            .expect("must get delegator 2 payout")
    };

    let validator_1_expected_payout = {
        let total_delegator_payout = delegator_1_expected_payout + delegator_2_expected_payout;
        let validator_share = expected_total_reward - Ratio::from(total_delegator_payout);
        validator_share.to_integer()
    };

    let validator_1_actual_payout = {
        let vaildator_stake_before = U512::from(VALIDATOR_1_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();

        validator_stake_after - vaildator_stake_before
    };

    assert_eq!(
        validator_1_actual_payout, validator_1_expected_payout,
        "rhs {}",
        validator_1_expected_payout
    );

    let delegator_1_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_1_actual_payout, delegator_1_expected_payout);

    let delegator_2_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_2_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_2_actual_payout, delegator_2_expected_payout);

    // Subsequently, there should be no more rewards
    let validator_1_balance = {
        withdraw_bid(
            &mut builder,
            *VALIDATOR_1_ADDR,
            VALIDATOR_1.clone(),
            validator_1_actual_payout + U512::from(VALIDATOR_1_STAKE),
        );
        assert!(get_validator_bid(&mut builder, VALIDATOR_1.clone()).is_none());
        U512::zero()
    };
    assert_eq!(validator_1_balance, U512::zero());

    let delegator_1_balance = {
        assert!(
            get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone()).is_none(),
            "validator withdrawing full stake also removes delegator 1 reinvested funds"
        );
        U512::zero()
    };
    assert_eq!(delegator_1_balance, U512::zero());

    let delegator_2_balance = {
        assert!(
            get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone()).is_none(),
            "validator withdrawing full stake also removes delegator 2 reinvested funds"
        );
        U512::zero()
    };
    assert!(delegator_2_balance.is_zero());

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == validator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_2_expected_payout
    ));
}

#[ignore]
#[test]
fn should_withdraw_bids_after_distribute() {
    const VALIDATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const DELEGATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const DELEGATOR_2_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const TOTAL_DELEGATOR_STAKE: u64 = DELEGATOR_1_STAKE + DELEGATOR_2_STAKE;
    const TOTAL_STAKE: u64 = VALIDATOR_1_STAKE + TOTAL_DELEGATOR_STAKE;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    let total_payout = builder.base_round_reward(None, protocol_version);

    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let rate = builder.round_seigniorage_rate(None, protocol_version);

    let expected_total_reward = rate * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    assert_eq!(total_payout, expected_total_reward_integer);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(protocol_version)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();
        assert!(
            builder.step(step_request).is_success(),
            "must execute step successfully"
        );
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_actual_payout = {
        let validator_stake_before = U512::from(VALIDATOR_1_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();

        validator_stake_after - validator_stake_before
    };

    let delegators_share = {
        let commission_rate = Ratio::new(
            U512::from(VALIDATOR_1_DELEGATION_RATE),
            U512::from(DELEGATION_RATE_DENOMINATOR),
        );
        let reward_multiplier =
            Ratio::new(U512::from(TOTAL_DELEGATOR_STAKE), U512::from(TOTAL_STAKE));
        let delegator_reward = expected_total_reward
            .checked_mul(&reward_multiplier)
            .expect("must get delegator reward");
        let commission = delegator_reward
            .checked_mul(&commission_rate)
            .expect("must get commission");
        delegator_reward.checked_sub(&commission).unwrap()
    };

    let delegator_1_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_1_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        reward_multiplier
            .checked_mul(&delegators_share)
            .map(|ratio| ratio.to_integer())
            .unwrap()
    };

    let delegator_2_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_2_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        reward_multiplier
            .checked_mul(&delegators_share)
            .map(|ratio| ratio.to_integer())
            .unwrap()
    };

    let validator_1_expected_payout = {
        let total_delegator_payout = delegator_1_expected_payout + delegator_2_expected_payout;
        let validator_share = expected_total_reward - Ratio::from(total_delegator_payout);
        validator_share.to_integer()
    };
    assert_eq!(
        validator_1_actual_payout, validator_1_expected_payout,
        "rhs {}",
        validator_1_expected_payout
    );

    let delegator_1_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };

    assert_eq!(delegator_1_actual_payout, delegator_1_expected_payout);

    let delegator_2_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_2_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_stake_after - delegator_stake_before
    };

    assert_eq!(delegator_2_actual_payout, delegator_2_expected_payout);

    let delegator_1_unstaked_amount = {
        assert!(
            get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone()).is_some(),
            "delegator 1 should have a stake"
        );
        let undelegate_amount = U512::from(DELEGATOR_1_STAKE) + delegator_1_actual_payout;

        undelegate(
            &mut builder,
            *DELEGATOR_1_ADDR,
            DELEGATOR_1.clone(),
            VALIDATOR_1.clone(),
            undelegate_amount,
        );
        assert!(
            get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone()).is_none(),
            "delegator 1 did not unstake full expected amount"
        );
        delegator_1_actual_payout
    };
    assert!(
        !delegator_1_unstaked_amount.is_zero(),
        "should have unstaked more than zero"
    );

    let delegator_2_unstaked_amount = {
        assert!(
            get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone()).is_some(),
            "delegator 2 should have a stake"
        );
        let undelegate_amount = U512::from(DELEGATOR_2_STAKE) + delegator_2_actual_payout;
        undelegate(
            &mut builder,
            *DELEGATOR_2_ADDR,
            DELEGATOR_2.clone(),
            VALIDATOR_1.clone(),
            undelegate_amount,
        );
        assert!(
            get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone()).is_none(),
            "delegator 2 did not unstake full expected amount"
        );
        delegator_2_actual_payout
    };
    assert!(
        !delegator_2_unstaked_amount.is_zero(),
        "should have unstaked more than zero"
    );

    let validator_1_balance = {
        assert!(
            get_validator_bid(&mut builder, VALIDATOR_1.clone()).is_some(),
            "validator 1 should have a stake"
        );
        let withdraw_bid_amount = validator_1_actual_payout + U512::from(VALIDATOR_1_STAKE);
        withdraw_bid(
            &mut builder,
            *VALIDATOR_1_ADDR,
            VALIDATOR_1.clone(),
            withdraw_bid_amount,
        );

        assert!(get_validator_bid(&mut builder, VALIDATOR_1.clone()).is_none());

        withdraw_bid_amount
    };
    assert!(!validator_1_balance.is_zero());

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == validator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_1_expected_payout
    ));
}

#[ignore]
#[allow(unused)]
// #[test]
fn should_distribute_rewards_after_restaking_delegated_funds() {
    const VALIDATOR_1_STAKE: u64 = 7_000_000_000_000_000;
    const DELEGATOR_1_STAKE: u64 = 5_000_000_000_000_000;
    const DELEGATOR_2_STAKE: u64 = 5_000_000_000_000_000;
    const TOTAL_DELEGATOR_STAKE: u64 = DELEGATOR_1_STAKE + DELEGATOR_2_STAKE;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let initial_rate = builder.round_seigniorage_rate(None, protocol_version);
    let initial_round_reward = builder.base_round_reward(None, protocol_version);

    let initial_expected_reward_rate = initial_rate * initial_supply;
    assert_eq!(
        initial_round_reward,
        initial_expected_reward_rate.to_integer()
    );

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    // we need to crank forward because our validator is not a genesis validator
    builder.advance_eras_by_default_auction_delay();

    let mut era = builder.get_era();
    let mut round_reward = initial_round_reward;
    let mut total_supply = initial_supply;
    let mut expected_reward_rate = initial_expected_reward_rate;
    let mut validator_stake = U512::from(VALIDATOR_1_STAKE);
    let mut delegator_1_stake = U512::from(DELEGATOR_1_STAKE);
    let mut delegator_2_stake = U512::from(DELEGATOR_2_STAKE);
    let mut total_delegator_stake = U512::from(TOTAL_DELEGATOR_STAKE);
    let mut total_stake = U512::from(VALIDATOR_1_STAKE + TOTAL_DELEGATOR_STAKE);
    for idx in 0..10 {
        let rewards = {
            let mut rewards = BTreeMap::new();
            rewards.insert(VALIDATOR_1.clone(), round_reward);
            rewards
        };

        let result = builder.distribute(None, protocol_version, rewards, Timestamp::now().millis());
        assert!(result.is_success(), "failed to distribute {:?}", result);
        builder.advance_era();
        let current_era = builder.get_era();
        assert_eq!(
            era.successor(),
            current_era,
            "unexpected era {:?}",
            current_era
        );
        era = current_era;

        let updated_round_reward = builder.base_round_reward(None, protocol_version);
        round_reward = updated_round_reward;

        let updated_validator_stake = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        assert!(
            updated_validator_stake > validator_stake,
            "validator stake should go up"
        );
        let updated_delegator_1_stake =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        assert!(
            updated_delegator_1_stake > delegator_1_stake,
            "delegator 1 stake should go up"
        );
        let updated_delegator_2_stake =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        assert!(
            updated_delegator_2_stake > delegator_2_stake,
            "delegator 2 stake should go up was: {:?} is: {:?}",
            delegator_2_stake,
            updated_delegator_2_stake,
        );
        let updated_total_delegator_stake = updated_delegator_1_stake + updated_delegator_2_stake;
        assert!(
            updated_total_delegator_stake > total_delegator_stake,
            "total delegator stake should go up"
        );
        total_delegator_stake = updated_total_delegator_stake;
        let updated_total_stake = updated_validator_stake + updated_total_delegator_stake;
        assert!(
            updated_total_stake > total_stake,
            "total stake should go up"
        );

        let delegators_share = {
            let commission_rate = Ratio::new(
                U512::from(VALIDATOR_1_DELEGATION_RATE),
                U512::from(DELEGATION_RATE_DENOMINATOR),
            );
            let reward_multiplier = Ratio::new(updated_total_delegator_stake, updated_total_stake);
            let delegator_reward = expected_reward_rate
                .checked_mul(&reward_multiplier)
                .expect("should get delegator reward ratio");
            let commission = delegator_reward
                .checked_mul(&commission_rate)
                .expect("must get delegator reward");
            delegator_reward.checked_sub(&commission).unwrap()
        };

        let delegator_1_expected_payout = {
            let reward_multiplier =
                Ratio::new(updated_delegator_1_stake, updated_total_delegator_stake);
            delegators_share
                .checked_mul(&reward_multiplier)
                .map(|ratio| ratio.to_integer())
                .expect("must get delegator 1 reward")
        };

        let delegator_2_expected_payout = {
            let reward_multiplier =
                Ratio::new(updated_delegator_2_stake, updated_total_delegator_stake);
            delegators_share
                .checked_mul(&reward_multiplier)
                .map(|ratio| ratio.to_integer())
                .expect("must get delegator 2 reward")
        };

        let validator_1_actual_payout = updated_validator_stake - validator_stake;

        let validator_1_expected_payout = (expected_reward_rate
            - Ratio::from(delegator_1_expected_payout + delegator_2_expected_payout))
        .to_integer();
        assert_eq!(validator_1_actual_payout, validator_1_expected_payout);

        let delegator_1_actual_payout = updated_delegator_1_stake - delegator_1_stake;
        assert_eq!(delegator_1_actual_payout, delegator_1_expected_payout);

        let delegator_2_actual_payout = updated_delegator_2_stake - delegator_2_stake;
        assert_eq!(delegator_2_actual_payout, delegator_2_expected_payout);

        let updated_era_info = get_era_info(&mut builder);

        assert!(matches!(
            updated_era_info.select(VALIDATOR_1.clone()).next(),
            Some(SeigniorageAllocation::Validator { validator_public_key, amount })
            if *validator_public_key == *VALIDATOR_1 && *amount == validator_1_expected_payout
        ));

        assert!(matches!(
            updated_era_info.select(DELEGATOR_1.clone()).next(),
            Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
            if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_expected_payout
        ));

        assert!(matches!(
            updated_era_info.select(DELEGATOR_2.clone()).next(),
            Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
            if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_2_expected_payout
        ));

        // Next round of rewards
        let updated_supply = builder.total_supply(protocol_version, None);
        assert!(updated_supply > total_supply);
        total_supply = updated_supply;

        let updated_rate = builder.round_seigniorage_rate(None, protocol_version);
        expected_reward_rate = updated_rate * total_supply;

        // lets churn the bids just to have some fun
        let undelegate_amount = delegator_1_expected_payout - 1;
        let undelegate_result = builder.bidding(
            None,
            protocol_version,
            (*DELEGATOR_1_ADDR).into(),
            AuctionMethod::Undelegate {
                validator: VALIDATOR_1.clone(),
                delegator: DELEGATOR_1.clone(),
                amount: undelegate_amount,
            },
        );
        assert!(undelegate_result.is_success(), "{:?}", undelegate_result);
        builder.commit_transforms(builder.get_post_state_hash(), undelegate_result.effects());
        delegator_1_stake =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());

        let updelegate_amount = U512::from(1_000_000);
        let updelegate_result = builder.bidding(
            None,
            protocol_version,
            (*DELEGATOR_2_ADDR).into(),
            AuctionMethod::Delegate {
                max_delegators_per_validator: u32::MAX,
                validator: VALIDATOR_1.clone(),
                delegator: DELEGATOR_2.clone(),
                amount: updelegate_amount,
            },
        );
        assert!(updelegate_result.is_success(), "{:?}", updelegate_result);
        builder.commit_transforms(builder.get_post_state_hash(), undelegate_result.effects());
        delegator_2_stake =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());

        let auction_method = {
            let amount = U512::from(10_000_000);
            if idx % 2 == 0 {
                AuctionMethod::AddBid {
                    public_key: VALIDATOR_1.clone(),
                    amount,
                    delegation_rate: 0,
                    minimum_delegation_amount: updelegate_amount.as_u64(),
                    maximum_delegation_amount: updelegate_amount.as_u64(),
                }
            } else {
                AuctionMethod::WithdrawBid {
                    public_key: VALIDATOR_1.clone(),
                    amount,
                }
            }
        };
        let bid_flip_result = builder.bidding(
            None,
            protocol_version,
            (*VALIDATOR_1_ADDR).into(),
            auction_method,
        );
        assert!(bid_flip_result.is_success(), "{:?}", bid_flip_result);
        builder.commit_transforms(builder.get_post_state_hash(), undelegate_result.effects());
        validator_stake = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();

        total_stake = validator_stake + delegator_1_stake + delegator_2_stake;
    }
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_half() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000_000_000;
    const TOTAL_DELEGATOR_STAKE: u64 = DELEGATOR_1_STAKE + DELEGATOR_2_STAKE;
    const TOTAL_STAKE: u64 = VALIDATOR_1_STAKE + TOTAL_DELEGATOR_STAKE;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let total_payout = builder.base_round_reward(None, protocol_version);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();
    assert_eq!(total_payout, expected_total_reward_integer);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        assert!(
            builder.step(step_request).is_success(),
            "must execute step successfully"
        );
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let delegators_share = {
        let commission_rate = Ratio::new(
            U512::from(VALIDATOR_1_DELEGATION_RATE),
            U512::from(DELEGATION_RATE_DENOMINATOR),
        );
        let reward_multiplier =
            Ratio::new(U512::from(TOTAL_DELEGATOR_STAKE), U512::from(TOTAL_STAKE));
        let delegator_reward = expected_total_reward
            .checked_mul(&reward_multiplier)
            .expect("must get delegator reward");
        let commission = delegator_reward
            .checked_mul(&commission_rate)
            .expect("must get commission");
        delegator_reward.checked_sub(&commission).unwrap()
    };

    let delegator_1_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_1_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        delegators_share
            .checked_mul(&reward_multiplier)
            .map(|ratio| ratio.to_integer())
            .unwrap()
    };

    let delegator_2_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_2_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        delegators_share
            .checked_mul(&reward_multiplier)
            .map(|ratio| ratio.to_integer())
            .unwrap()
    };

    let validator_1_expected_payout = {
        let total_delegator_payout = delegator_1_expected_payout + delegator_2_expected_payout;
        let validators_part = expected_total_reward - Ratio::from(total_delegator_payout);
        validators_part.to_integer()
    };

    let validator_1_actual_payout = {
        let validator_stake_before = U512::from(VALIDATOR_1_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };

    assert_eq!(validator_1_actual_payout, validator_1_expected_payout);

    let delegator_1_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_1_actual_payout, delegator_1_expected_payout);

    let delegator_2_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_2_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_2_actual_payout, delegator_2_expected_payout);

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == validator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_2_expected_payout
    ));
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_full() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), expected_total_reward_integer);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_updated_stake = {
        let validator_stake_before = U512::from(VALIDATOR_1_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };
    let expected_validator_1_balance =
        (expected_total_reward * Ratio::from(U512::one())).to_integer();
    assert_eq!(validator_1_updated_stake, expected_validator_1_balance);

    let delegator_1_updated_stake = {
        let validator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let validator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        validator_stake_after - validator_stake_before
    };
    let expected_delegator_1_balance = U512::zero();
    assert_eq!(delegator_1_updated_stake, expected_delegator_1_balance);

    let delegator_2_balance = {
        let validator_stake_before = U512::from(DELEGATOR_2_STAKE);
        let validator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        validator_stake_after - validator_stake_before
    };
    let expected_delegator_2_balance = U512::zero();
    assert_eq!(delegator_2_balance, expected_delegator_2_balance);

    let total_payout = validator_1_updated_stake + delegator_1_updated_stake + delegator_2_balance;
    assert_eq!(total_payout, expected_total_reward_integer);

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == expected_validator_1_balance
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == expected_delegator_1_balance
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == expected_delegator_1_balance
    ));
}

#[ignore]
#[test]
fn should_distribute_uneven_delegation_rate_zero() {
    const VALIDATOR_1_STAKE: u64 = 200_000_000_000;
    const DELEGATOR_1_STAKE: u64 = 600_000_000_000;
    const DELEGATOR_2_STAKE: u64 = 800_000_000_000;
    const TOTAL_DELEGATOR_STAKE: u64 = DELEGATOR_1_STAKE + DELEGATOR_2_STAKE;
    const TOTAL_STAKE: u64 = VALIDATOR_1_STAKE + TOTAL_DELEGATOR_STAKE;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let total_payout = builder.base_round_reward(None, protocol_version);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();
    assert_eq!(total_payout, expected_total_reward_integer);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        assert!(
            builder.step(step_request).is_success(),
            "must execute step successfully"
        );
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let delegators_share = {
        let commission_rate = Ratio::new(
            U512::from(VALIDATOR_1_DELEGATION_RATE),
            U512::from(DELEGATION_RATE_DENOMINATOR),
        );
        let reward_multiplier =
            Ratio::new(U512::from(TOTAL_DELEGATOR_STAKE), U512::from(TOTAL_STAKE));
        let delegator_reward = expected_total_reward
            .checked_mul(&reward_multiplier)
            .expect("must get delegator reward");
        let commission = delegator_reward
            .checked_mul(&commission_rate)
            .expect("must get commission");
        delegator_reward.checked_sub(&commission).unwrap()
    };

    let delegator_1_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_1_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        delegators_share
            .checked_mul(&reward_multiplier)
            .map(|ratio| ratio.to_integer())
            .unwrap()
    };

    let delegator_2_expected_payout = {
        let reward_multiplier = Ratio::new(
            U512::from(DELEGATOR_2_STAKE),
            U512::from(TOTAL_DELEGATOR_STAKE),
        );
        delegators_share
            .checked_mul(&reward_multiplier)
            .map(|ratio| ratio.to_integer())
            .unwrap()
    };

    let validator_1_expected_payout = {
        let total_delegator_payout = delegator_1_expected_payout + delegator_2_expected_payout;
        let validators_part = expected_total_reward - Ratio::from(total_delegator_payout);
        validators_part.to_integer()
    };

    let validator_1_updated_stake = {
        let validator_stake_before = U512::from(VALIDATOR_1_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };
    assert_eq!(validator_1_updated_stake, validator_1_expected_payout);

    let delegator_1_updated_stake = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_1_updated_stake, delegator_1_expected_payout);

    let delegator_2_updated_stake = {
        let delegator_stake_before = U512::from(DELEGATOR_2_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_2_updated_stake, delegator_2_expected_payout);

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == validator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_2_expected_payout
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

    const DELEGATOR_1_STAKE: u64 = 6_000_000_000_000;
    const DELEGATOR_2_STAKE: u64 = 8_000_000_000_000;
    const DELEGATOR_3_STAKE: u64 = 2_000_000_000_000;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_2_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2.clone(),
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_3_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    let delegator_3_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_3_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_3_STAKE),
            ARG_VALIDATOR => VALIDATOR_2.clone(),
            ARG_DELEGATOR => DELEGATOR_3.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let total_payout = builder.base_round_reward(None, protocol_version);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();
    assert_eq!(total_payout, expected_total_reward_integer);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        assert!(
            builder.step(step_request).is_success(),
            "must execute step successfully"
        );
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);

    // Validator 1 distribution
    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_actual_payout = {
        let validator_stake_before = U512::from(VALIDATOR_1_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };

    let delegator_1_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };

    let delegator_2_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_2_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_stake_after - delegator_stake_before
    };

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_1 && *amount == validator_1_actual_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_actual_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_2_actual_payout
    ));

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_2.clone(), total_payout);

    // Validator 2 distribution
    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_2_actual_payout = {
        let validator_stake_before = U512::from(VALIDATOR_2_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_2.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };

    let delegator_3_actual_payout = {
        let delegator_stake_before = U512::from(DELEGATOR_3_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_2.clone(), DELEGATOR_3.clone());
        delegator_stake_after - delegator_stake_before
    };

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_2 && *amount == validator_2_actual_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_3.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_3 && *amount == delegator_3_actual_payout
    ));

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_3.clone(), total_payout);

    // Validator 3 distribution
    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_3_actual_payout = {
        let validator_stake_before = U512::from(VALIDATOR_3_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_3.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(VALIDATOR_3.clone()).next(),
        Some(SeigniorageAllocation::Validator { validator_public_key, amount })
        if *validator_public_key == *VALIDATOR_3 && *amount == validator_3_actual_payout
    ));
}

#[ignore]
#[test]
fn should_distribute_with_multiple_validators_and_shared_delegator() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000_000_000;
    const VALIDATOR_2_STAKE: u64 = 1_000_000_000_000;
    const VALIDATOR_3_STAKE: u64 = 1_000_000_000_000;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    const DELEGATOR_1_STAKE: u64 = 1_000_000_000_000;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2.clone(),
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3.clone(),
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_2.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_validator_3_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_3.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let total_payout = builder.base_round_reward(None, protocol_version);
    let expected_total_reward = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();
    assert_eq!(total_payout, expected_total_reward_integer);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();
        assert!(
            builder.step(step_request).is_success(),
            "must execute step successfully"
        );
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);
    rewards.insert(VALIDATOR_2.clone(), total_payout);
    rewards.insert(VALIDATOR_3.clone(), total_payout);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_delegator_1_share = {
        let total_reward = &Ratio::from(expected_total_reward_integer);

        let validator_1_total_stake = VALIDATOR_1_STAKE + DELEGATOR_1_STAKE;

        let delegator_total_stake = U512::from(DELEGATOR_1_STAKE);
        let commission_rate = Ratio::new(
            U512::from(DELEGATION_RATE),
            U512::from(DELEGATION_RATE_DENOMINATOR),
        );
        let reward_multiplier =
            Ratio::new(delegator_total_stake, U512::from(validator_1_total_stake));
        let delegator_reward = total_reward
            .checked_mul(&reward_multiplier)
            .expect("must get delegator reward");
        let commission = delegator_reward
            .checked_mul(&commission_rate)
            .expect("must get commission");
        delegator_reward.checked_sub(&commission).unwrap()
    }
    .to_integer();

    let validator_1_actual_payout = {
        let validator_balance_before = U512::from(VALIDATOR_1_STAKE);
        let validator_balance_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_balance_after - validator_balance_before
    };

    let validator_1_expected_payout = {
        let validator_share = expected_total_reward;
        let validator_portion = validator_share - Ratio::from(validator_1_delegator_1_share);
        validator_portion.to_integer()
    };
    assert_eq!(validator_1_actual_payout, validator_1_expected_payout);

    let validator_2_delegator_1_share = {
        let validator_2_total_stake = VALIDATOR_2_STAKE + DELEGATOR_1_STAKE;

        let total_reward = &Ratio::from(expected_total_reward.to_integer());

        let delegator_total_stake = U512::from(DELEGATOR_1_STAKE);
        let commission_rate = Ratio::new(
            U512::from(DELEGATION_RATE),
            U512::from(DELEGATION_RATE_DENOMINATOR),
        );
        let reward_multiplier =
            Ratio::new(delegator_total_stake, U512::from(validator_2_total_stake));
        let delegator_reward = total_reward
            .checked_mul(&reward_multiplier)
            .expect("must get delegator reward");
        let commission = delegator_reward
            .checked_mul(&commission_rate)
            .expect("must get commission");
        delegator_reward.checked_sub(&commission).unwrap()
    }
    .to_integer();

    let validator_2_actual_payout = {
        let validator_balance_before = U512::from(VALIDATOR_2_STAKE);
        let validator_balance_after = get_validator_bid(&mut builder, VALIDATOR_2.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_balance_after - validator_balance_before
    };
    let validator_2_expected_payout = {
        let validator_share = expected_total_reward;
        let validator_portion = validator_share - Ratio::from(validator_2_delegator_1_share);
        validator_portion.to_integer()
    };
    assert_eq!(validator_2_actual_payout, validator_2_expected_payout);

    let validator_3_delegator_1_share = {
        let validator_3_total_stake = VALIDATOR_3_STAKE + DELEGATOR_1_STAKE;

        let total_reward = &Ratio::from(expected_total_reward.to_integer());

        let delegator_total_stake = U512::from(DELEGATOR_1_STAKE);
        let commission_rate = Ratio::new(
            U512::from(DELEGATION_RATE),
            U512::from(DELEGATION_RATE_DENOMINATOR),
        );
        let reward_multiplier =
            Ratio::new(delegator_total_stake, U512::from(validator_3_total_stake));
        let delegator_reward = total_reward
            .checked_mul(&reward_multiplier)
            .expect("must get delegator reward");
        let commission = delegator_reward
            .checked_mul(&commission_rate)
            .expect("must get commission");
        delegator_reward.checked_sub(&commission).unwrap()
    }
    .to_integer();

    let validator_3_actual_payout = {
        let validator_balance_before = U512::from(VALIDATOR_3_STAKE);
        let validator_balance_after = get_validator_bid(&mut builder, VALIDATOR_3.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_balance_after - validator_balance_before
    };
    let validator_3_expected_payout = {
        let validator_share = expected_total_reward;
        let validator_portion = validator_share - Ratio::from(validator_3_delegator_1_share);
        validator_portion.to_integer()
    };
    assert_eq!(validator_3_actual_payout, validator_3_expected_payout);

    let delegator_1_validator_1_updated_stake = {
        let delegator_balance_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_balance_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_balance_after - delegator_balance_before
    };

    assert_eq!(
        delegator_1_validator_1_updated_stake,
        validator_1_delegator_1_share
    );

    let delegator_1_validator_2_updated_stake = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_2.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(
        delegator_1_validator_2_updated_stake,
        validator_2_delegator_1_share
    );

    let delegator_1_validator_3_updated_stake = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_3.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(
        delegator_1_validator_3_updated_stake,
        validator_3_delegator_1_share
    );
}

#[ignore]
#[test]
fn should_increase_total_supply_after_distribute() {
    const VALIDATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const VALIDATOR_2_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const VALIDATOR_3_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    const DELEGATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_2_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_2.clone(),
        },
    )
    .build();

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_3_STAKE),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_3.clone(),
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_2.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_validator_3_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_3.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    let post_genesis_supply = builder.total_supply(protocol_version, None);

    assert_eq!(
        initial_supply, post_genesis_supply,
        "total supply should remain unchanged prior to first distribution"
    );

    // run auction
    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let post_auction_supply = builder.total_supply(protocol_version, None);
    assert_eq!(
        initial_supply, post_auction_supply,
        "total supply should remain unchanged regardless of auction"
    );

    let total_payout = U512::from(1_000_000_000_000_u64);

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);
    rewards.insert(VALIDATOR_2.clone(), total_payout);
    rewards.insert(VALIDATOR_3.clone(), total_payout);

    for _ in 0..5 {
        let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
            *SYSTEM_ADDR,
            builder.get_auction_contract_hash(),
            METHOD_DISTRIBUTE,
            runtime_args! {
                ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
                ARG_REWARDS_MAP => rewards.clone()
            },
        )
        .build();

        builder.exec(distribute_request).expect_success().commit();

        let post_distribute_supply = builder.total_supply(protocol_version, None);
        assert!(
            initial_supply < post_distribute_supply,
            "total supply should increase after distribute ({} >= {})",
            initial_supply,
            post_distribute_supply
        );
    }
}

#[ignore]
#[test]
fn should_not_create_purses_during_distribute() {
    const VALIDATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;

    const DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR / 2;

    const DELEGATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    let delegator_3_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_3_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_3.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        delegator_3_fund_request,
        validator_1_add_bid_request,
        delegator_1_validator_1_delegate_request,
        delegator_2_validator_1_delegate_request,
        delegator_3_validator_1_delegate_request,
    ];

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    let post_genesis_supply = builder.total_supply(protocol_version, None);

    assert_eq!(
        initial_supply, post_genesis_supply,
        "total supply should remain unchanged prior to first distribution"
    );

    // run auction
    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let post_auction_supply = builder.total_supply(protocol_version, None);
    assert_eq!(
        initial_supply, post_auction_supply,
        "total supply should remain unchanged regardless of auction"
    );

    let total_payout = U512::from(1_000_000_000_000_u64);

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), total_payout);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    let number_of_purses_before_distribute = builder.get_balance_keys().len();

    builder.exec(distribute_request).commit().expect_success();

    let number_of_purses_after_distribute = builder.get_balance_keys().len();

    assert_eq!(
        number_of_purses_after_distribute,
        number_of_purses_before_distribute
    );

    let post_distribute_supply = builder.total_supply(protocol_version, None);
    assert!(
        initial_supply < post_distribute_supply,
        "total supply should increase after distribute ({} >= {})",
        initial_supply,
        post_distribute_supply
    );
}

#[ignore]
#[test]
fn should_distribute_delegation_rate_full_after_upgrading() {
    const VALIDATOR_1_STAKE: u64 = 1_000_000_000_000;
    const DELEGATOR_1_STAKE: u64 = 1_000_000_000_000;
    const DELEGATOR_2_STAKE: u64 = 1_000_000_000_000;

    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = DELEGATION_RATE_DENOMINATOR;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
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
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
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

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    // initial token supply
    let initial_supply = builder.total_supply(protocol_version, None);
    let expected_total_reward_before = *GENESIS_ROUND_SEIGNIORAGE_RATE * initial_supply;
    let expected_total_reward_integer = expected_total_reward_before.to_integer();

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.advance_era();
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), expected_total_reward_integer);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let validator_1_stake_before = {
        let validator_stake_before = U512::from(VALIDATOR_1_STAKE);
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };

    let expected_validator_1_payout_before =
        (expected_total_reward_before * Ratio::from(U512::one())).to_integer();
    assert_eq!(validator_1_stake_before, expected_validator_1_payout_before);

    let delegator_1_stake_before = {
        let delegator_stake_before = U512::from(DELEGATOR_1_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };
    let expected_delegator_1_payout_before = U512::zero();
    assert_eq!(delegator_1_stake_before, expected_delegator_1_payout_before);

    let delegator_2_stake_before = {
        let delegator_stake_before = U512::from(DELEGATOR_2_STAKE);
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_stake_after - delegator_stake_before
    };
    let expected_delegator_2_balance = U512::zero();
    assert_eq!(delegator_2_stake_before, expected_delegator_2_balance);

    let total_payout_before =
        validator_1_stake_before + delegator_1_stake_before + delegator_2_stake_before;
    assert_eq!(total_payout_before, expected_total_reward_integer);

    //
    // Update round seigniorage rate into 50% of default value
    //
    let new_seigniorage_multiplier = Ratio::new_raw(1, 2);
    let new_round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE * new_seigniorage_multiplier;

    let old_protocol_version = DEFAULT_PROTOCOL_VERSION;
    let sem_ver = old_protocol_version.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let mut upgrade_request = {
        const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(old_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_round_seigniorage_rate(new_round_seigniorage_rate)
            .build()
    };

    builder.upgrade(&mut upgrade_request);

    let initial_supply = builder.total_supply(protocol_version, None);

    for _ in 0..5 {
        builder.advance_era();
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let new_round_seigniorage_rate = {
        let (numer, denom) = new_round_seigniorage_rate.into();
        Ratio::new(numer.into(), denom.into())
    };

    let expected_total_reward_after = new_round_seigniorage_rate * initial_supply;

    let mut rewards = BTreeMap::new();
    rewards.insert(
        VALIDATOR_1.clone(),
        expected_total_reward_after.to_integer(),
    );
    assert!(
        builder
            .distribute(None, new_protocol_version, rewards, timestamp_millis)
            .is_success(),
        "must distribute"
    );

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), expected_total_reward_integer);

    let validator_1_balance_after = {
        let validator_staked_amount = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_staked_amount - validator_1_stake_before - U512::from(VALIDATOR_1_STAKE)
    };
    let expected_validator_1_balance_after =
        (expected_total_reward_after * Ratio::from(U512::one())).to_integer();
    assert_eq!(
        validator_1_balance_after,
        expected_validator_1_balance_after
    );

    let delegator_1_balance_after = {
        let delegator_staked_amount =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_staked_amount - delegator_1_stake_before - U512::from(DELEGATOR_1_STAKE)
    };
    let expected_delegator_1_balance_after = U512::zero();
    assert_eq!(
        delegator_1_balance_after,
        expected_delegator_1_balance_after
    );

    let delegator_2_balance_after = {
        let delegator_staked_amount =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_staked_amount - delegator_2_stake_before - U512::from(DELEGATOR_2_STAKE)
    };
    let expected_delegator_2_balance_after = U512::zero();
    assert_eq!(
        delegator_2_balance_after,
        expected_delegator_2_balance_after
    );

    let expected_total_reward_after = expected_total_reward_after.to_integer();

    let total_payout_after =
        validator_1_balance_after + delegator_1_balance_after + delegator_2_balance_after;
    assert_eq!(total_payout_after, expected_total_reward_after);

    // expected amount after reducing the seigniorage rate is lower than the first amount
    assert!(expected_validator_1_payout_before > expected_validator_1_balance_after);
    assert!(total_payout_before > total_payout_after);
}

// In this test, we set up a validator and a delegator, then the delegator delegates to the
// validator. We step forward one era (auction delay is 3 eras) and then fully undelegate. We expect
// that there is no bonding purse for this delegator / validator pair. This test should prove that
// if you undelegate before your delegation would receive rewards from a validator, you will no
// longer be delegated, as expected.
#[ignore]
#[test]
fn should_not_restake_after_full_unbond() {
    const DELEGATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // advance past the initial auction delay due to special condition of post-genesis behavior.

    builder.advance_eras_by_default_auction_delay();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder
        .exec(validator_1_fund_request)
        .expect_success()
        .commit();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder
        .exec(delegator_1_fund_request)
        .expect_success()
        .commit();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    builder
        .exec(validator_1_add_bid_request)
        .expect_success()
        .commit();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    builder
        .exec(delegator_1_validator_1_delegate_request)
        .expect_success()
        .commit();

    builder.advance_era();

    let delegator = get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());

    assert!(delegator.is_some());
    assert_eq!(
        delegator.unwrap().staked_amount(),
        U512::from(DELEGATOR_1_STAKE)
    );

    builder.advance_era();

    // undelegate in the era right after we delegated.
    undelegate(
        &mut builder,
        *DELEGATOR_1_ADDR,
        DELEGATOR_1.clone(),
        VALIDATOR_1.clone(),
        U512::from(DELEGATOR_1_STAKE),
    );
    let delegator = get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
    assert!(delegator.is_none());

    let withdraws = builder.get_unbonds();
    let unbonding_purses = withdraws
        .get(&DELEGATOR_1_ADDR)
        .expect("should have validator entry");
    let delegator_unbond_amount = unbonding_purses
        .iter()
        .find(|up| *up.unbonder_public_key() == DELEGATOR_1.clone())
        .expect("should be unbonding purse");

    assert_eq!(
        *delegator_unbond_amount.amount(),
        U512::from(DELEGATOR_1_STAKE),
        "unbond purse amount should match staked amount"
    );

    // step until validator receives rewards.
    builder.advance_eras_by(2);

    // validator receives rewards after this step.

    builder.advance_era();

    // Delegator should not remain delegated even though they were eligible for rewards in the
    // second era.
    let delegator = get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
    assert!(delegator.is_none());
}

// In this test, we set up a delegator and a validator, the delegator delegates to the validator.
// We then undelegate during the first era where the delegator would be eligible to receive rewards
// for their delegation and expect that there is no bonding purse for the delegator / validator pair
// and that the delegator does not remain delegated to the validator as expected.
#[ignore]
#[test]
fn delegator_full_unbond_during_first_reward_era() {
    const DELEGATOR_1_STAKE: u64 = DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const VALIDATOR_1_STAKE: u64 = 1_000_000;
    const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // advance past the initial auction delay due to special condition of post-genesis behavior.
    builder.advance_eras_by_default_auction_delay();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder
        .exec(validator_1_fund_request)
        .expect_success()
        .commit();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder
        .exec(delegator_1_fund_request)
        .expect_success()
        .commit();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    builder
        .exec(validator_1_add_bid_request)
        .expect_success()
        .commit();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    builder
        .exec(delegator_1_validator_1_delegate_request)
        .expect_success()
        .commit();

    // first step after funding, adding bid and delegating.
    builder.advance_era();

    let delegator = get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone())
        .expect("should be delegator");

    assert_eq!(delegator.staked_amount(), U512::from(DELEGATOR_1_STAKE));

    // step until validator receives rewards.
    builder.advance_eras_by(3);

    // assert that the validator should indeed receive rewards and that
    // the delegator is scheduled to receive rewards this era.

    let auction_hash = builder.get_auction_contract_hash();
    let seigniorage_snapshot: SeigniorageRecipientsSnapshot = builder.get_value(
        EntityAddr::System(auction_hash.value()),
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    );

    let validator_seigniorage = seigniorage_snapshot
        .get(&builder.get_era())
        .expect("should be seigniorage for era")
        .get(&VALIDATOR_1)
        .expect("should be validator seigniorage for era");

    let delegator_seigniorage = validator_seigniorage
        .delegator_stake()
        .get(&DELEGATOR_1)
        .expect("should be delegator seigniorage");
    assert_eq!(*delegator_seigniorage, U512::from(DELEGATOR_1_STAKE));

    // undelegate in the first era that the delegator will receive rewards.
    undelegate(
        &mut builder,
        *DELEGATOR_1_ADDR,
        DELEGATOR_1.clone(),
        VALIDATOR_1.clone(),
        U512::from(DELEGATOR_1_STAKE),
    );
    let delegator = get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
    assert!(delegator.is_none());

    let withdraws = builder.get_unbonds();
    let unbonding_purses = withdraws
        .get(&DELEGATOR_1_ADDR)
        .expect("should have validator entry");
    let delegator_unbond_amount = unbonding_purses
        .iter()
        .find(|up| *up.unbonder_public_key() == DELEGATOR_1.clone())
        .expect("should be unbonding purse");

    assert_eq!(
        *delegator_unbond_amount.amount(),
        U512::from(DELEGATOR_1_STAKE),
        "unbond purse amount should match staked amount"
    );

    // validator receives rewards after this step.
    builder.advance_era();

    // Delegator should not remain delegated even though they were eligible for rewards in the
    // second era.
    let delegator = get_delegator_bid(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
    assert!(delegator.is_none());
}
