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
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    },
    bytesrepr::FromBytes,
    CLTyped, ContractHash, ProtocolVersion, PublicKey,
};

const ACCOUNT_1_PK: PublicKey = PublicKey::Ed25519([200; 32]);
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([201; 32]);
const ACCOUNT_1_BALANCE: u64 = 10_000_000;
const ACCOUNT_1_BOND: u64 = 100_000;

const ACCOUNT_2_PK: PublicKey = PublicKey::Ed25519([202; 32]);
const ACCOUNT_2_ADDR: AccountHash = AccountHash::new([203; 32]);
const ACCOUNT_2_BALANCE: u64 = 25_000_000;
const ACCOUNT_2_BOND: u64 = 200_000;

fn get_value<T: FromBytes + CLTyped>(
    builder: &mut InMemoryWasmTestBuilder,
    contract_hash: ContractHash,
    name: &str,
) -> T {
    let contract = builder
        .get_contract(contract_hash)
        .expect("should have contract");
    let key = contract
        .named_keys()
        .get(name)
        .expect("should have bid purses");
    let stored_value = builder.query(None, *key, &[]).expect("should query");
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
        .with_reward_item(RewardItem::new(ACCOUNT_2_PK, 100))
        .build();

    let auction_hash = builder.get_auction_contract_hash();

    let _bid_purses_before_slashing: BidPurses =
        get_value(&mut builder, auction_hash, BID_PURSES_KEY);

    // TODO: this is currently not possible due to a bug in the slashing logic
    //       the genesis validators are not slashable due to an oversight in initialization
    // assert!(
    //     bid_purses_before_slashing.contains_key(&ACCOUNT_1_PK),
    //     "should contain slashed validator)"
    // );

    let before_auction_seigniorage: SeigniorageRecipientsSnapshot = get_value(
        &mut builder,
        auction_hash,
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    );

    // TODO: capture state prior to reward pay out

    builder.step(step_request);

    let bid_purses_after_slashing: BidPurses =
        get_value(&mut builder, auction_hash, BID_PURSES_KEY);

    assert!(
        !bid_purses_after_slashing.contains_key(&ACCOUNT_1_PK),
        "should not contain slashed validator)"
    );

    let after_auction_seigniorage: SeigniorageRecipientsSnapshot = get_value(
        &mut builder,
        auction_hash,
        SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
    );

    assert!(
        !before_auction_seigniorage
            .keys()
            .all(|key| after_auction_seigniorage.contains_key(key)),
        "run auction should have changed seigniorage keys"
    );

    // TODO: capture state after reward pay out & verify outcome
}
