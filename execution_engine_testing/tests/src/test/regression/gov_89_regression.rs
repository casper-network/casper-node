use std::{
    collections::BTreeSet,
    convert::TryInto,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, InMemoryWasmTestBuilder, StepRequestBuilder, DEFAULT_ACCOUNTS,
};
use casper_execution_engine::{
    core::engine_state::{
        genesis::GenesisValidator, GenesisAccount, RewardItem, SlashItem, StepSuccess,
    },
    shared::transform::Transform,
};
use casper_types::{
    system::auction::{
        Bids, DelegationRate, SeigniorageRecipientsSnapshot, BLOCK_REWARD,
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    },
    CLValue, EraId, Key, Motes, ProtocolVersion, PublicKey, SecretKey, StoredValue, U512,
};

static ACCOUNT_1_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([200; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_1_BALANCE: u64 = 100_000_000;
const ACCOUNT_1_BOND: u64 = 100_000_000;

static ACCOUNT_2_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([202; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_2_BALANCE: u64 = 200_000_000;
const ACCOUNT_2_BOND: u64 = 200_000_000;

fn initialize_builder() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp
    };
    let run_genesis_request = utils::create_run_genesis_request(accounts);
    builder.run_genesis(&run_genesis_request);
    builder
}

#[ignore]
#[test]
fn should_not_create_any_purse() {
    let mut builder = initialize_builder();

    let mut now = SystemTime::now();
    let eras_end_timestamp_millis_1 = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    now += Duration::from_secs(60 * 60);
    let eras_end_timestamp_millis_2 = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    assert!(eras_end_timestamp_millis_2 > eras_end_timestamp_millis_1);

    let step_request_1 = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_slash_item(SlashItem::new(ACCOUNT_1_PK.clone()))
        .with_reward_item(RewardItem::new(ACCOUNT_1_PK.clone(), BLOCK_REWARD / 2))
        .with_reward_item(RewardItem::new(ACCOUNT_2_PK.clone(), BLOCK_REWARD / 2))
        .with_next_era_id(EraId::from(1))
        .with_era_end_timestamp_millis(eras_end_timestamp_millis_1.as_millis().try_into().unwrap())
        .build();

    let auction_hash = builder.get_auction_contract_hash();

    let before_auction_seigniorage: SeigniorageRecipientsSnapshot =
        builder.get_value(auction_hash, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY);

    let bids_before_slashing: Bids = builder.get_bids();
    assert!(
        bids_before_slashing.contains_key(&ACCOUNT_1_PK),
        "should have entry in the genesis bids table {:?}",
        bids_before_slashing
    );

    let bids_before_slashing: Bids = builder.get_bids();
    assert!(
        bids_before_slashing.contains_key(&ACCOUNT_1_PK),
        "should have entry in bids table before slashing {:?}",
        bids_before_slashing
    );

    let StepSuccess {
        execution_journal: journal_1,
        ..
    } = builder.step(step_request_1).expect("should execute step");

    let bids_after_slashing: Bids = builder.get_bids();
    let account_1_bid = bids_after_slashing.get(&ACCOUNT_1_PK).unwrap();
    assert!(account_1_bid.inactive());
    assert!(account_1_bid.staked_amount().is_zero());

    let bids_after_slashing: Bids = builder.get_bids();
    assert_ne!(
        bids_before_slashing, bids_after_slashing,
        "bids table should be different before and after slashing"
    );

    // seigniorage snapshot should have changed after auction
    let after_auction_seigniorage: SeigniorageRecipientsSnapshot =
        builder.get_value(auction_hash, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY);
    assert!(
        !before_auction_seigniorage
            .keys()
            .all(|key| after_auction_seigniorage.contains_key(key)),
        "run auction should have changed seigniorage keys"
    );

    let step_request_2 = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_slash_item(SlashItem::new(ACCOUNT_1_PK.clone()))
        .with_reward_item(RewardItem::new(ACCOUNT_1_PK.clone(), BLOCK_REWARD / 2))
        .with_reward_item(RewardItem::new(ACCOUNT_2_PK.clone(), BLOCK_REWARD / 2))
        .with_next_era_id(EraId::from(2))
        .with_era_end_timestamp_millis(eras_end_timestamp_millis_2.as_millis().try_into().unwrap())
        .build();

    let StepSuccess {
        execution_journal: journal_2,
        ..
    } = builder.step(step_request_2).expect("should execute step");

    let cl_u512_zero = CLValue::from_t(U512::zero()).unwrap();

    let balances_1: BTreeSet<Key> = journal_1
        .into_iter()
        .filter_map(|(key, transform)| match transform {
            Transform::Write(StoredValue::CLValue(cl_value))
                if key.as_balance().is_some() && cl_value == cl_u512_zero =>
            {
                Some(key)
            }
            _ => None,
        })
        .collect();

    assert_eq!(balances_1.len(), 0, "distribute should not create purses");

    let balances_2: BTreeSet<Key> = journal_2
        .into_iter()
        .filter_map(|(key, transform)| match transform {
            Transform::Write(StoredValue::CLValue(cl_value))
                if key.as_balance().is_some() && cl_value == cl_u512_zero =>
            {
                Some(key)
            }
            _ => None,
        })
        .collect();

    assert_eq!(balances_2.len(), 0, "distribute should not create purses");

    let common_keys: BTreeSet<_> = balances_1.intersection(&balances_2).collect();
    assert_eq!(common_keys.len(), 0, "there should be no commmon Key::Balance keys with Transfer::Write(0) in two distinct step requests");
}
