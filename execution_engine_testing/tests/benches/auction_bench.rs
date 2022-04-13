use std::{path::Path, time::Duration};

use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion, Throughput,
};
use rand::Rng;
use tempfile::TempDir;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_AUCTION_DELAY, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE,
    DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY, DEFAULT_WASM_CONFIG,
    MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR,
};
use casper_execution_engine::{
    core::engine_state::{
        genesis::GenesisValidator, run_genesis_request::RunGenesisRequest, EngineConfig,
        ExecConfig, ExecuteRequest, GenesisAccount, RewardItem,
    },
    shared::system_config::auction_costs::DEFAULT_DELEGATE_COST,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::auction::{self},
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

/// Runs genesis, creates system, validator and delegator accounts, and funds the system account and
/// delegator accounts.
fn run_genesis_and_create_initial_accounts(
    data_dir: &Path,
    validator_keys: &[PublicKey],
    delegator_accounts: Vec<AccountHash>,
) -> LmdbWasmTestBuilder {
    let engine_config = EngineConfig::default();
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir, engine_config);

    let mut genesis_accounts = vec![
        GenesisAccount::account(
            DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            Motes::new(U512::MAX), // all the monies
            None,
        ),
        GenesisAccount::account(
            DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            None,
        ),
    ];
    for validator in validator_keys {
        genesis_accounts.push(GenesisAccount::account(
            validator.clone(),
            Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)),
            Some(GenesisValidator::new(
                Motes::new(U512::from(VALIDATOR_BID_AMOUNT)),
                DELEGATION_RATE,
            )),
        ))
    }
    let run_genesis_request =
        create_run_genesis_request(validator_keys.len() as u32 + 2, genesis_accounts);
    builder.run_genesis(&run_genesis_request);

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

fn create_run_genesis_request(
    validator_slots: u32,
    genesis_accounts: Vec<GenesisAccount>,
) -> RunGenesisRequest {
    let exec_config = {
        ExecConfig::new(
            genesis_accounts,
            *DEFAULT_WASM_CONFIG,
            *DEFAULT_SYSTEM_CONFIG,
            validator_slots,
            DEFAULT_AUCTION_DELAY,
            DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
            DEFAULT_ROUND_SEIGNIORAGE_RATE,
            DEFAULT_UNBONDING_DELAY,
            DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        )
    };
    RunGenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        exec_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    )
}

fn setup_bench_run_auction(
    group: &mut BenchmarkGroup<WallTime>,
    validator_count: usize,
    delegator_count: usize,
) {
    // Setup delegator public keys
    let delegator_keys = generate_public_keys(delegator_count);
    let validator_keys = generate_public_keys(validator_count);

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = run_genesis_and_create_initial_accounts(
        data_dir.path(),
        &validator_keys,
        delegator_keys
            .iter()
            .map(|pk| pk.to_account_hash())
            .collect::<Vec<_>>(),
    );

    let contract_hash = builder.get_auction_contract_hash();
    let mut next_validator_iter = validator_keys.iter().cycle();
    for delegator_public_key in delegator_keys {
        let balance = builder
            .get_public_key_balance_result(delegator_public_key.clone())
            .motes()
            .cloned()
            .unwrap();

        assert_eq!(U512::from(DELEGATOR_INITIAL_BALANCE), balance);

        let delegation_amount = U512::from(DELEGATION_AMOUNT);
        let delegator_account_hash = delegator_public_key.to_account_hash();
        let next_validator_key = next_validator_iter
            .next()
            .expect("should produce values forever");
        let delegate = create_delegate_request(
            delegator_public_key,
            next_validator_key.clone(),
            delegation_amount,
            delegator_account_hash,
            contract_hash,
        );
        builder.exec(delegate);
        builder.expect_success();
        builder.commit();
        builder.clear_results();
    }

    let mut era_end_timestamp = TIMESTAMP_INCREMENT_MILLIS;

    group.bench_function(
        format!(
            "run_auction/validators/{}/delegators/{}",
            validator_count, delegator_count
        ),
        |b| {
            b.iter(|| {
                era_end_timestamp += TIMESTAMP_INCREMENT_MILLIS;
                step_and_run_auction(&mut builder, &validator_keys)
            })
        },
    );
}

fn create_delegate_request(
    delegator_public_key: PublicKey,
    next_validator_key: PublicKey,
    delegation_amount: U512,
    delegator_account_hash: AccountHash,
    contract_hash: casper_types::ContractHash,
) -> ExecuteRequest {
    let entry_point = auction::METHOD_DELEGATE;
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator_public_key,
        auction::ARG_VALIDATOR => next_validator_key,
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
    ExecuteRequestBuilder::new().push_deploy(deploy).build()
}

fn generate_public_keys(key_count: usize) -> Vec<PublicKey> {
    let mut ret = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        let bytes: [u8; SecretKey::ED25519_LENGTH] = rand::random();
        let secret_key = SecretKey::ed25519_from_bytes(&bytes).unwrap();
        let public_key = PublicKey::from(&secret_key);
        ret.push(public_key);
    }
    ret
}

fn step_and_run_auction(builder: &mut LmdbWasmTestBuilder, validator_keys: &[PublicKey]) {
    let mut step_request_builder = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0);

    for validator in validator_keys {
        step_request_builder =
            step_request_builder.with_reward_item(RewardItem::new(validator.clone(), 1));
    }
    let step_request = step_request_builder
        .with_next_era_id(builder.get_era() + 1)
        .build();
    builder.step(step_request).expect("should step");
}

pub fn auction_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("auction_bench_group");

    /// Total number of validators, total number of delegators. Delegators will be spread
    /// round-robin over the validators.
    const VALIDATOR_DELEGATOR_COUNTS: [(usize, usize); 4] =
        [(100, 8000), (150, 8000), (100, 10000), (150, 10000)];
    for (validator_count, delegator_count) in VALIDATOR_DELEGATOR_COUNTS {
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(30));
        group.throughput(Throughput::Elements(1));
        println!(
            "Starting bench of {} validators and {} delegators",
            validator_count, delegator_count
        );
        setup_bench_run_auction(&mut group, validator_count, delegator_count);
        println!("Ended bench");
    }
    group.finish();
}

criterion_group!(benches, auction_bench);
criterion_main!(benches);
