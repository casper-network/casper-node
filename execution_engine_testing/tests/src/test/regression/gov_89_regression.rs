use std::{
    collections::BTreeSet,
    convert::TryInto,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, LmdbWasmTestBuilder, StepRequestBuilder, DEFAULT_ACCOUNTS,
};
use casper_storage::data_access_layer::{SlashItem, StepResult};
use casper_types::{
    execution::TransformKindV2,
    system::auction::{
        BidsExt, DelegationRate, SeigniorageRecipientsSnapshotV2,
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    },
    CLValue, EntityAddr, EraId, GenesisAccount, GenesisValidator, Key, Motes, ProtocolVersion,
    PublicKey, SecretKey, StoredValue, U512,
};

static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([200; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_1_BALANCE: u64 = 100_000_000;
const ACCOUNT_1_BOND: u64 = 100_000_000;

static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([202; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_2_BALANCE: u64 = 200_000_000;
const ACCOUNT_2_BOND: u64 = 200_000_000;

fn initialize_builder() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PUBLIC_KEY.clone(),
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PUBLIC_KEY.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp
    };
    let run_genesis_request = utils::create_run_genesis_request(accounts);
    builder.run_genesis(run_genesis_request);
    builder
}

#[ignore]
#[test]
fn should_not_create_any_purse() {
    let mut builder = initialize_builder();
    let auction_hash = builder.get_auction_contract_hash();

    let mut now = SystemTime::now();
    let eras_end_timestamp_millis_1 = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    now += Duration::from_secs(60 * 60);
    let eras_end_timestamp_millis_2 = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    assert!(eras_end_timestamp_millis_2 > eras_end_timestamp_millis_1);

    let step_request_1 = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_slash_item(SlashItem::new(ACCOUNT_1_PUBLIC_KEY.clone()))
        .with_next_era_id(EraId::from(1))
        .with_era_end_timestamp_millis(eras_end_timestamp_millis_1.as_millis().try_into().unwrap())
        .build();

    let before_auction_seigniorage: SeigniorageRecipientsSnapshotV2 = builder.get_value(
        EntityAddr::System(auction_hash.value()),
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    );

    let bids_before_slashing = builder.get_bids();
    assert!(
        bids_before_slashing.contains_validator_public_key(&ACCOUNT_1_PUBLIC_KEY),
        "should have entry in the genesis bids table {:?}",
        bids_before_slashing
    );

    let effects_1 = match builder.step(step_request_1) {
        StepResult::Failure(_) => {
            panic!("step_request_1: Failure")
        }
        StepResult::RootNotFound => {
            panic!("step_request_1: RootNotFound")
        }
        StepResult::Success { effects, .. } => effects,
    };

    assert!(
        builder
            .query(
                None,
                Key::Unbond(ACCOUNT_1_PUBLIC_KEY.to_account_hash()),
                &[],
            )
            .is_err(),
        "slash does not unbond"
    );

    let bids_after_slashing = builder.get_bids();
    assert!(
        !bids_after_slashing.contains_validator_public_key(&ACCOUNT_1_PUBLIC_KEY),
        "should not have entry after slashing {:?}",
        bids_after_slashing
    );

    let bids_after_slashing = builder.get_bids();
    let account_1_bid = bids_after_slashing.validator_bid(&ACCOUNT_1_PUBLIC_KEY);
    assert!(account_1_bid.is_none());

    let bids_after_slashing = builder.get_bids();
    assert_ne!(
        bids_before_slashing, bids_after_slashing,
        "bids table should be different before and after slashing"
    );

    // seigniorage snapshot should have changed after auction
    let after_auction_seigniorage: SeigniorageRecipientsSnapshotV2 = builder.get_value(
        EntityAddr::System(auction_hash.value()),
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    );
    assert!(
        !after_auction_seigniorage
            .keys()
            .all(|key| before_auction_seigniorage.contains_key(key)),
        "run auction should have changed seigniorage keys"
    );

    let step_request_2 = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_slash_item(SlashItem::new(ACCOUNT_1_PUBLIC_KEY.clone()))
        .with_next_era_id(EraId::from(2))
        .with_era_end_timestamp_millis(eras_end_timestamp_millis_2.as_millis().try_into().unwrap())
        .build();

    let effects_2 = match builder.step(step_request_2) {
        StepResult::RootNotFound | StepResult::Failure(_) => {
            panic!("step_request_2: failed to step")
        }
        StepResult::Success { effects, .. } => effects,
    };

    let cl_u512_zero = CLValue::from_t(U512::zero()).unwrap();

    let balances_1: BTreeSet<Key> = effects_1
        .transforms()
        .iter()
        .filter_map(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::CLValue(cl_value))
                if transform.key().as_balance().is_some() && cl_value == &cl_u512_zero =>
            {
                Some(*transform.key())
            }
            _ => None,
        })
        .collect();

    assert_eq!(balances_1.len(), 0, "distribute should not create purses");

    let balances_2: BTreeSet<Key> = effects_2
        .transforms()
        .iter()
        .filter_map(|transform| match transform.kind() {
            TransformKindV2::Write(StoredValue::CLValue(cl_value))
                if transform.key().as_balance().is_some() && cl_value == &cl_u512_zero =>
            {
                Some(*transform.key())
            }
            _ => None,
        })
        .collect();

    assert_eq!(balances_2.len(), 0, "distribute should not create purses");

    let common_keys: BTreeSet<_> = balances_1.intersection(&balances_2).collect();
    assert_eq!(common_keys.len(), 0, "there should be no commmon Key::Balance keys with Transfer::Write(0) in two distinct step requests");
}
