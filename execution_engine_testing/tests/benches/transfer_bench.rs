use std::time::Duration;

use criterion::{
    criterion_group, criterion_main,
    measurement::{Measurement, WallTime},
    BenchmarkGroup, Criterion, Throughput,
};
use tempfile::TempDir;

use casper_engine_test_support::{
    transfer,
    transfer::{BLOCK_TRANSFER_COUNT, TARGET_ADDR, TRANSFER_BATCH_SIZE},
    LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::U512;

pub fn transfer_to_existing_accounts(group: &mut BenchmarkGroup<WallTime>, should_commit: bool) {
    let target_account = TARGET_ADDR;
    let bootstrap_accounts = vec![target_account];

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    transfer::create_initial_accounts_and_run_genesis(
        &mut builder,
        bootstrap_accounts.clone(),
        U512::one(),
    );

    group.bench_function(
        format!(
            "transfer_to_existing_account_multiple_execs/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            b.iter(|| {
                // Execute multiple deploys with multiple exec requests
                transfer::transfer_to_account_multiple_execs(
                    &mut builder,
                    target_account,
                    should_commit,
                )
            })
        },
    );

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    transfer::create_initial_accounts_and_run_genesis(
        &mut builder,
        bootstrap_accounts,
        U512::one(),
    );

    group.bench_function(
        format!(
            "transfer_to_existing_account_multiple_deploys_per_exec/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            b.iter(|| {
                // Execute multiple deploys with a single exec request
                transfer::transfer_to_account_multiple_deploys(
                    &mut builder,
                    target_account,
                    should_commit,
                )
            })
        },
    );
}

// Generate multiple purses as well as transfer requests between them with the specified count.
pub fn multiple_native_transfers<M>(
    group: &mut BenchmarkGroup<M>,
    transfer_count: usize,
    purse_count: usize,
    use_scratch: bool,
) where
    M: Measurement,
{
    let target_account = TARGET_ADDR;
    let bootstrap_accounts = vec![target_account];

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    transfer::create_initial_accounts_and_run_genesis(
        &mut builder,
        bootstrap_accounts,
        U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
    );

    let purse_amount = U512::one();

    let purses = transfer::create_test_purses(&mut builder, target_account, 100, purse_amount);

    let exec_requests =
        transfer::create_multiple_native_transfers_to_purses(*DEFAULT_ACCOUNT_ADDR, 2500, &purses);

    let criterion_metric_name = std::any::type_name::<M>();

    group.bench_function(
        format!(
            "type:{}/transfers:{}/purses:{}/metric:{}",
            if use_scratch { "scratch" } else { "lmdb" },
            transfer_count,
            purse_count,
            criterion_metric_name,
        ),
        |b| {
            b.iter(|| {
                transfer::transfer_to_account_multiple_native_transfers(
                    &mut builder,
                    &exec_requests,
                    use_scratch,
                );
            });
        },
    );
}

pub fn transfer_to_existing_purses(group: &mut BenchmarkGroup<WallTime>, should_commit: bool) {
    let target_account = TARGET_ADDR;
    let bootstrap_accounts = vec![target_account];

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    transfer::create_initial_accounts_and_run_genesis(
        &mut builder,
        bootstrap_accounts.clone(),
        U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) * 10,
    );

    let purse_amount = U512::one();
    let purses = transfer::create_test_purses(&mut builder, target_account, 1, purse_amount);

    group.bench_function(
        format!(
            "transfer_to_purse_multiple_execs/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            let target_purse = purses[0];
            b.iter(|| {
                // Execute multiple deploys with multiple exec request
                transfer::transfer_to_purse_multiple_execs(
                    &mut builder,
                    target_purse,
                    should_commit,
                )
            })
        },
    );

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    transfer::create_initial_accounts_and_run_genesis(
        &mut builder,
        bootstrap_accounts,
        U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) * 10,
    );
    let purses = transfer::create_test_purses(&mut builder, TARGET_ADDR, 1, U512::one());

    group.bench_function(
        format!(
            "transfer_to_purse_multiple_deploys_per_exec/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            let target_purse = purses[0];
            b.iter(|| {
                // Execute multiple deploys with a single exec request
                transfer::transfer_to_purse_multiple_deploys(
                    &mut builder,
                    target_purse,
                    should_commit,
                )
            })
        },
    );
}

pub fn native_transfer_bench<M>(c: &mut Criterion<M>)
where
    M: Measurement,
{
    let mut group: BenchmarkGroup<'_, M> = c.benchmark_group("tps_native");

    // Minimum number of samples and measurement times to decrease the total time of this benchmark.
    // This may or may not decrease the quality of the numbers.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(60));

    // Measure by elements where one element per second is one transaction per second
    group.throughput(Throughput::Elements(BLOCK_TRANSFER_COUNT as u64));

    for purse_count in [50, 100] {
        for transfer_count in [500, 1500, 2500usize] {
            // baseline, one deploy per exec request
            multiple_native_transfers(&mut group, transfer_count, purse_count, true);
            multiple_native_transfers(&mut group, transfer_count, purse_count, false);
        }
    }

    group.finish();
}

pub fn transfer_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("tps");

    // Minimum number of samples and measurement times to decrease the total time of this benchmark.
    // This may or may not decrease the quality of the numbers.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    // Measure by elements where one element per second is one transaction per second
    group.throughput(Throughput::Elements(TRANSFER_BATCH_SIZE));

    // Transfers to existing accounts, no commits
    transfer_to_existing_accounts(&mut group, false);

    // Transfers to existing purses, no commits
    transfer_to_existing_purses(&mut group, false);

    // Transfers to existing accounts, with commits
    transfer_to_existing_accounts(&mut group, true);

    // Transfers to existing purses, with commits
    transfer_to_existing_purses(&mut group, true);

    group.finish();
}

criterion_group!(
    name = native_transfer_benches;
    config = Criterion::default().with_measurement(WallTime);
    targets = native_transfer_bench<WallTime>
);
criterion_group!(
    name = benches;
    config = Criterion::default().with_measurement(WallTime);
    targets = transfer_bench
);
criterion_main!(benches, native_transfer_benches);
