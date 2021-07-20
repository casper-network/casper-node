use std::convert::TryFrom;

use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::internal::{
    utils, InMemoryWasmTestBuilder, StepRequestBuilder, WasmTestBuilder, DEFAULT_ACCOUNTS,
};
use casper_execution_engine::{
    core::engine_state::{
        genesis::{GenesisAccount, GenesisValidator},
        RewardItem, SlashItem,
    },
    shared::motes::Motes,
    storage::global_state::in_memory::InMemoryGlobalState,
};
use casper_types::{
    system::{
        auction::{
            Bids, DelegationRate, SeigniorageRecipientsSnapshot, BLOCK_REWARD,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
        },
        mint::TOTAL_SUPPLY_KEY,
    },
    CLValue, ContractHash, EraId, Key, ProtocolVersion, PublicKey, SecretKey, U512,
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

fn get_named_key(
    builder: &mut InMemoryWasmTestBuilder,
    contract_hash: ContractHash,
    name: &str,
) -> Key {
    *builder
        .get_contract(contract_hash)
        .expect("should have contract")
        .named_keys()
        .get(name)
        .expect("should have bid purses")
}

fn initialize_builder() -> WasmTestBuilder<InMemoryGlobalState> {
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

/// Should be able to step slashing, rewards, and run auction.
#[ignore]
#[test]
fn should_step() {
    let mut builder = initialize_builder();

    let step_request = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_slash_item(SlashItem::new(ACCOUNT_1_PK.clone()))
        .with_reward_item(RewardItem::new(ACCOUNT_1_PK.clone(), BLOCK_REWARD / 2))
        .with_reward_item(RewardItem::new(ACCOUNT_2_PK.clone(), BLOCK_REWARD / 2))
        .with_next_era_id(EraId::from(1))
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

    builder.step(step_request);

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
}

/// Should be able to step slashing, rewards, and run auction.
#[ignore]
#[test]
fn should_adjust_total_supply() {
    let mut builder = initialize_builder();
    let maybe_post_state_hash = Some(builder.get_post_state_hash());

    let mint_hash = builder.get_mint_contract_hash();

    // should check total supply before step
    let total_supply_key = get_named_key(&mut builder, mint_hash, TOTAL_SUPPLY_KEY)
        .into_uref()
        .expect("should be uref");

    let starting_total_supply = CLValue::try_from(
        builder
            .query(maybe_post_state_hash, total_supply_key.into(), &[])
            .expect("should have total supply"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");

    // slash
    let step_request = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_slash_item(SlashItem::new(ACCOUNT_1_PK.clone()))
        .with_slash_item(SlashItem::new(ACCOUNT_2_PK.clone()))
        .with_reward_item(RewardItem::new(ACCOUNT_1_PK.clone(), 0))
        .with_reward_item(RewardItem::new(ACCOUNT_2_PK.clone(), BLOCK_REWARD / 2))
        .with_next_era_id(EraId::from(1))
        .build();

    builder.step(step_request);
    let maybe_post_state_hash = Some(builder.get_post_state_hash());

    // should check total supply after step
    let modified_total_supply = CLValue::try_from(
        builder
            .query(maybe_post_state_hash, total_supply_key.into(), &[])
            .expect("should have total supply"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");

    assert!(
        modified_total_supply < starting_total_supply,
        "total supply should be reduced due to slashing"
    );
}
