use std::time::Duration;

use casper_execution_engine::core::engine_state::EngineConfig;
use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion, Throughput,
};
use tempfile::TempDir;

use casper_engine_test_support::{
    auction::{DELEGATOR_INITIAL_BALANCE, TIMESTAMP_INCREMENT_MILLIS},
    DbWasmTestBuilder,
};
use casper_types::U512;

fn setup_bench_run_auction(
    group: &mut BenchmarkGroup<WallTime>,
    validator_count: usize,
    delegator_count: usize,
    rocksdb_opts: rocksdb::Options,
) {
    // Setup delegator public keys
    let delegator_keys = casper_engine_test_support::auction::generate_public_keys(delegator_count);
    let validator_keys = casper_engine_test_support::auction::generate_public_keys(validator_count);

    let data_dir = TempDir::new().expect("should create temp dir");
    let engine_config = EngineConfig::default();
    let mut builder =
        DbWasmTestBuilder::new_with_config(data_dir.as_ref(), engine_config, rocksdb_opts);
    casper_engine_test_support::auction::run_genesis_and_create_initial_accounts(
        &mut builder,
        &validator_keys,
        delegator_keys
            .iter()
            .map(|pk| pk.to_account_hash())
            .collect::<Vec<_>>(),
        U512::from(DELEGATOR_INITIAL_BALANCE),
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

        let delegation_amount = U512::from(42);
        let delegator_account_hash = delegator_public_key.to_account_hash();
        let next_validator_key = next_validator_iter
            .next()
            .expect("should produce values forever");
        let delegate = casper_engine_test_support::auction::create_delegate_request(
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
                casper_engine_test_support::auction::step_and_run_auction(
                    &mut builder,
                    &validator_keys,
                );
            })
        },
    );
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
        setup_bench_run_auction(
            &mut group,
            validator_count,
            delegator_count,
            casper_execution_engine::rocksdb_defaults(),
        );
        println!("Ended bench");
    }
    group.finish();
}

criterion_group!(benches, auction_bench);
criterion_main!(benches);
