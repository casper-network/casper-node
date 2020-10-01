use casper_engine_test_support::internal::{
    utils, InMemoryWasmTestBuilder, RewardItem, SlashItem, StepRequestBuilder, WasmTestBuilder,
    DEFAULT_ACCOUNTS,
};
use casper_execution_engine::{
    core::engine_state::genesis::GenesisAccount, shared::motes::Motes,
    storage::global_state::in_memory::InMemoryGlobalState,
};
use casper_types::{
    account::AccountHash,
    auction::{
        BidPurses, SeigniorageRecipientsSnapshot, BID_PURSES_KEY,
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, VALIDATOR_REWARD_PURSE,
    },
    bytesrepr::FromBytes,
    CLTyped, ContractHash, Key, ProtocolVersion, PublicKey,
};

const ACCOUNT_1_PK: PublicKey = PublicKey::Ed25519([200; 32]);
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([200; 32]);
const ACCOUNT_1_BALANCE: u64 = 10_000_000;
const ACCOUNT_1_BOND: u64 = 100_000;

const ACCOUNT_2_PK: PublicKey = PublicKey::Ed25519([202; 32]);
const ACCOUNT_2_ADDR: AccountHash = AccountHash::new([202; 32]);
const ACCOUNT_2_BALANCE: u64 = 25_000_000;
const ACCOUNT_2_BOND: u64 = 200_000;

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

fn get_value<T: FromBytes + CLTyped>(builder: &mut InMemoryWasmTestBuilder, key: Key) -> T {
    let stored_value = builder.query(None, key, &[]).expect("should query");
    let cl_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should be cl value");
    let result: T = cl_value.into_t().expect("should convert");
    result
}

fn initialize_builder() -> WasmTestBuilder<InMemoryGlobalState> {
    let mut builder = InMemoryWasmTestBuilder::default();

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        let account_2 = GenesisAccount::new(
            ACCOUNT_2_PK,
            ACCOUNT_2_ADDR,
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Motes::new(ACCOUNT_2_BOND.into()),
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
        .with_slash_item(SlashItem::new(ACCOUNT_1_PK))
        // TODO: currently, it is necessary to add a reward item for account_1 due to the genesis
        // validator slashing immunity bug; when that bug is fixed, the below line adding
        // reward item for ACCOUNT_1_PK and this comment should be removed.
        .with_reward_item(RewardItem::new(ACCOUNT_1_PK, 100000))
        .with_reward_item(RewardItem::new(ACCOUNT_2_PK, 100000))
        .build();

    let auction_hash = builder.get_auction_contract_hash();

    let bid_purse_key = get_named_key(&mut builder, auction_hash, BID_PURSES_KEY);
    let reward_purse_key = get_named_key(&mut builder, auction_hash, VALIDATOR_REWARD_PURSE)
        .into_uref()
        .expect("should be uref");
    let auction_seigniorage_key = get_named_key(
        &mut builder,
        auction_hash,
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    );

    let before_balance = builder.get_purse_balance(reward_purse_key);
    let before_auction_seigniorage: SeigniorageRecipientsSnapshot =
        get_value(&mut builder, auction_seigniorage_key);

    // TODO: this is currently not possible due to a bug in the slashing logic
    //       the genesis validators are not slashable due to an oversight in initialization.
    // let _bid_purses_before_slashing: BidPurses = get_value(&mut builder, bid_purse_key);
    // assert!(
    //     bid_purses_before_slashing.contains_key(&ACCOUNT_1_PK),
    //     "should contain slashed validator)"
    // );

    builder.step(step_request);

    // bid purses should not have slashed validator after slashing
    let bid_purses_after_slashing: BidPurses = get_value(&mut builder, bid_purse_key);
    assert!(
        !bid_purses_after_slashing.contains_key(&ACCOUNT_1_PK),
        "should not contain slashed validator)"
    );

    // reward purse balance should not be the same after reward distribution
    let after_balance = builder.get_purse_balance(reward_purse_key);
    assert_ne!(
        before_balance, after_balance,
        "reward balance should change"
    );

    // seigniorage snapshot should have changed after auction
    let after_auction_seigniorage: SeigniorageRecipientsSnapshot =
        get_value(&mut builder, auction_seigniorage_key);
    assert!(
        !before_auction_seigniorage
            .keys()
            .all(|key| after_auction_seigniorage.contains_key(key)),
        "run auction should have changed seigniorage keys"
    );
}
