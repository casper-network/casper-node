use std::{path::Path, time::Duration};

use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion, Throughput,
};
use rand::Rng;
use tempfile::TempDir;

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_AUCTION_DELAY, DEFAULT_PROPOSER_PUBLIC_KEY, MINIMUM_ACCOUNT_CREATION_BALANCE,
    SYSTEM_ADDR,
};
use casper_execution_engine::{
    core::engine_state::{genesis::GenesisValidator, EngineConfig, GenesisAccount, RewardItem},
    shared::system_config::auction_costs::DEFAULT_DELEGATE_COST,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::auction::{self, Bids},
    Motes, ProtocolVersion, PublicKey, RuntimeArgs, SecretKey, U512,
};

const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";
const ARG_ID: &str = "id";

const DELEGATION_AMOUNT: u64 = 42;
const DELEGATION_RATE: u8 = 1;
const DELEGATOR_INITIAL_BALANCE: u64 = 500 * 1_000_000_000u64;

const VALIDATOR_BID_AMOUNT: u64 = 100;
const TIMESTAMP_INCREMENT_MILLIS: u64 = 30_000;

fn run_genesis_and_create_initial_accounts(
    data_dir: &Path,
    delegator_accounts: Vec<AccountHash>,
) -> LmdbWasmTestBuilder {
    let engine_config = EngineConfig::default();
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir, engine_config);

    let genesis_accounts = vec![
        GenesisAccount::account(
            DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            Motes::new(U512::MAX), // all the monies
            Some(GenesisValidator::new(
                Motes::new(U512::from(VALIDATOR_BID_AMOUNT)),
                DELEGATION_RATE,
            )),
            // None,
        ),
        GenesisAccount::account(
            DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            None,
        ),
    ];
    let run_genesis_request = utils::create_run_genesis_request(genesis_accounts);
    builder.run_genesis(&run_genesis_request);

    {
        // Setup the system account with enough cspr
        let transfer = ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                    ARG_TARGET => *SYSTEM_ADDR,
                    ARG_AMOUNT => MINIMUM_ACCOUNT_CREATION_BALANCE,
                    ARG_ID => <Option<u64>>::None,
            },
        )
        .build();
        builder.exec(transfer);
        builder.expect_success().commit();
    }

    for delegator_account in delegator_accounts {
        let transfer = ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                    ARG_TARGET => delegator_account,
                    ARG_AMOUNT => DELEGATOR_INITIAL_BALANCE,
                    ARG_ID => <Option<u64>>::None,
            },
        )
        .build();
        builder.exec(transfer);
        builder.expect_success().commit();
    }
    builder
}

fn setup_bench_run_auction(group: &mut BenchmarkGroup<WallTime>, delegator_count: usize) {
    // Setup delegator public keys
    let mut delegators = Vec::new();
    for _ in 0..delegator_count {
        let bytes: [u8; SecretKey::ED25519_LENGTH] = rand::random();
        let secret_key = SecretKey::ed25519_from_bytes(&bytes).unwrap();
        let public_key = PublicKey::from(&secret_key);
        delegators.push(public_key);
    }

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = run_genesis_and_create_initial_accounts(
        data_dir.path(),
        delegators
            .iter()
            .cloned()
            .map(|pk| pk.to_account_hash())
            .collect::<Vec<_>>(),
    );

    let bids: Bids = builder.get_bids();
    let active_bid = bids.get(&DEFAULT_ACCOUNT_PUBLIC_KEY.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(VALIDATOR_BID_AMOUNT),
    );
    assert_eq!(
        *active_bid.delegation_rate(),
        1,
        "unexpected delegation rate"
    );

    for delegator_public_key in delegators {
        let balance = builder
            .get_public_key_balance_result(delegator_public_key.clone())
            .motes()
            .cloned()
            .unwrap();

        assert_eq!(U512::from(DELEGATOR_INITIAL_BALANCE), balance);

        let delegation_amount = U512::from(DELEGATION_AMOUNT);
        let delegator_account_hash = delegator_public_key.to_account_hash();
        let delegate = {
            let contract_hash = builder.get_auction_contract_hash();
            let entry_point = auction::METHOD_DELEGATE;
            let args = runtime_args! {
                auction::ARG_DELEGATOR => delegator_public_key.clone(),
                auction::ARG_VALIDATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
                auction::ARG_AMOUNT => delegation_amount,
            };
            let mut rng = rand::thread_rng();
            let deploy_hash = rng.gen();

            let deploy = DeployItemBuilder::new()
                .with_address(delegator_account_hash)
                .with_stored_session_hash(contract_hash, entry_point, args)
                .with_empty_payment_bytes(
                    runtime_args! { ARG_AMOUNT => U512::from(DEFAULT_DELEGATE_COST), },
                )
                .with_authorization_keys(&[delegator_account_hash])
                .with_deploy_hash(deploy_hash)
                .build();

            ExecuteRequestBuilder::new().push_deploy(deploy)
        }
        .build();
        builder.exec(delegate);
        builder.expect_success();
        builder.commit();
    }

    let mut era_end_timestamp = TIMESTAMP_INCREMENT_MILLIS;

    // TODO: use add_bid to add non-genesis validator with stake

    // advance the auction past the auction delay so that the added validator will be present in the
    // auction
    for _ in 0..DEFAULT_AUCTION_DELAY {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era() + 1)
            // reward items for genesis validators need to be present...
            .with_reward_item(RewardItem::new(DEFAULT_ACCOUNT_PUBLIC_KEY.clone(), 1))
            .build();
        builder.step(step_request).expect("should step");
        builder.run_auction(era_end_timestamp, vec![]);
    }

    group.bench_function(format!("run_auction/delegators/{}", delegator_count), |b| {
        b.iter(|| {
            era_end_timestamp += TIMESTAMP_INCREMENT_MILLIS;
            step_and_run_auction(&mut builder, era_end_timestamp)
        })
    });
}

fn step_and_run_auction(builder: &mut LmdbWasmTestBuilder, timestamp: u64) {
    let step_request = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0)
        .with_reward_item(RewardItem::new(DEFAULT_ACCOUNT_PUBLIC_KEY.clone(), 1))
        .with_next_era_id(builder.get_era() + 1)
        .build();
    builder.step(step_request).expect("should step");
    builder.run_auction(timestamp, vec![]);
}

pub fn auction_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("auction_bench_group");
    group.sample_size(100);
    group.measurement_time(Duration::from_secs(120));
    for delegator_count in [250, 500, 1000] {
        group.throughput(Throughput::Elements(1));
        setup_bench_run_auction(&mut group, delegator_count as usize);
    }
    group.finish();
}

criterion_group!(benches, auction_bench);
criterion_main!(benches);
