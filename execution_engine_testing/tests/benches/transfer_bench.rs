use std::{path::Path, time::Duration};

use criterion::{
    criterion_group, criterion_main,
    measurement::{Measurement, WallTime},
    BenchmarkGroup, Criterion, Throughput,
};
use tempfile::TempDir;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::engine_state::{EngineConfig, ExecuteRequest};
use casper_types::{account::AccountHash, runtime_args, Key, RuntimeArgs, URef, U512};

const CONTRACT_CREATE_ACCOUNTS: &str = "create_accounts.wasm";
const CONTRACT_CREATE_PURSES: &str = "create_purses.wasm";
const CONTRACT_TRANSFER_TO_EXISTING_ACCOUNT: &str = "transfer_to_existing_account.wasm";
const CONTRACT_TRANSFER_TO_PURSE: &str = "transfer_to_purse.wasm";

/// Size of batch used in multiple execs benchmark, and multiple deploys per exec cases.
const TRANSFER_BATCH_SIZE: u64 = 3;
const TARGET_ADDR: AccountHash = AccountHash::new([127; 32]);
const ARG_AMOUNT: &str = "amount";
const ARG_ID: &str = "id";
const ARG_ACCOUNTS: &str = "accounts";
const ARG_TOTAL_PURSES: &str = "total_purses";
const ARG_TARGET: &str = "target";
const ARG_TARGET_PURSE: &str = "target_purse";

const BLOCK_TRANSFER_COUNT: usize = 2500;

/// Converts an integer into an array of type [u8; 32] by converting integer
/// into its big endian representation and embedding it at the end of the
/// range.
fn make_deploy_hash(i: u64) -> [u8; 32] {
    let mut result = [128; 32];
    result[32 - 8..].copy_from_slice(&i.to_be_bytes());
    result
}

fn bootstrap(data_dir: &Path, accounts: Vec<AccountHash>, amount: U512) -> LmdbWasmTestBuilder {
    let seed_amount = amount * accounts.len();
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_CREATE_ACCOUNTS,
        runtime_args! { ARG_ACCOUNTS => accounts, ARG_AMOUNT => seed_amount },
    )
    .build();

    let engine_config = EngineConfig::default();

    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir, engine_config);

    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .expect_success()
        .commit();

    builder
}

fn create_purses(
    builder: &mut LmdbWasmTestBuilder,
    source: AccountHash,
    total_purses: u64,
    purse_amount: U512,
) -> Vec<URef> {
    let seed_amount = purse_amount * total_purses;
    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_CREATE_PURSES,
        runtime_args! { ARG_TOTAL_PURSES => total_purses, ARG_AMOUNT => seed_amount },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    // Return creates purses for given account by filtering named keys
    let query_result = builder
        .query(None, Key::Account(source), &[])
        .expect("should query target");
    let account = query_result
        .as_account()
        .unwrap_or_else(|| panic!("result should be account but received {:?}", query_result));

    (0..total_purses)
        .map(|index| {
            let purse_lookup_key = format!("purse:{}", index);
            let purse_uref = account
                .named_keys()
                .get(&purse_lookup_key)
                .and_then(Key::as_uref)
                .unwrap_or_else(|| panic!("should get named key {} as uref", purse_lookup_key));
            *purse_uref
        })
        .collect()
}

/// Uses multiple exec requests with a single deploy to transfer tokens. Executes all transfers in
/// batch determined by value of TRANSFER_BATCH_SIZE.
fn transfer_to_account_multiple_execs(
    builder: &mut LmdbWasmTestBuilder,
    account: AccountHash,
    should_commit: bool,
) {
    let amount = U512::one();

    for _ in 0..TRANSFER_BATCH_SIZE {
        let exec_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_EXISTING_ACCOUNT,
            runtime_args! {
                ARG_TARGET => account,
                ARG_AMOUNT => amount,
            },
        )
        .build();

        let builder = builder.exec(exec_request).expect_success();
        if should_commit {
            builder.commit();
        }
    }
}

/// Executes multiple deploys per single exec with based on TRANSFER_BATCH_SIZE.
fn transfer_to_account_multiple_deploys(
    builder: &mut LmdbWasmTestBuilder,
    account: AccountHash,
    should_commit: bool,
) {
    let mut exec_builder = ExecuteRequestBuilder::new();

    for i in 0..TRANSFER_BATCH_SIZE {
        let deploy = DeployItemBuilder::default()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_session_code(
                CONTRACT_TRANSFER_TO_EXISTING_ACCOUNT,
                runtime_args! {
                    ARG_TARGET => account,
                    ARG_AMOUNT => U512::one(),
                },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash(make_deploy_hash(i)) // deploy_hash
            .build();
        exec_builder = exec_builder.push_deploy(deploy);
    }

    let exec_request = exec_builder.build();

    let builder = builder.exec(exec_request).expect_success();
    if should_commit {
        builder.commit();
    }
}

/// Uses multiple exec requests with a single deploy to transfer tokens from purse to purse.
/// Executes all transfers in batch determined by value of TRANSFER_BATCH_SIZE.
fn transfer_to_purse_multiple_execs(
    builder: &mut LmdbWasmTestBuilder,
    purse: URef,
    should_commit: bool,
) {
    let amount = U512::one();

    for _ in 0..TRANSFER_BATCH_SIZE {
        let exec_request = ExecuteRequestBuilder::standard(
            TARGET_ADDR,
            CONTRACT_TRANSFER_TO_PURSE,
            runtime_args! { ARG_TARGET_PURSE => purse, ARG_AMOUNT => amount },
        )
        .build();

        let builder = builder.exec(exec_request).expect_success();
        if should_commit {
            builder.commit();
        }
    }
}

/// Executes multiple deploys per single exec with based on TRANSFER_BATCH_SIZE.
fn transfer_to_purse_multiple_deploys(
    builder: &mut LmdbWasmTestBuilder,
    purse: URef,
    should_commit: bool,
) {
    let mut exec_builder = ExecuteRequestBuilder::new();

    for i in 0..TRANSFER_BATCH_SIZE {
        let deploy = DeployItemBuilder::default()
            .with_address(TARGET_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_session_code(
                CONTRACT_TRANSFER_TO_PURSE,
                runtime_args! { ARG_TARGET_PURSE => purse, ARG_AMOUNT => U512::one() },
            )
            .with_authorization_keys(&[TARGET_ADDR])
            .with_deploy_hash(make_deploy_hash(i)) // deploy_hash
            .build();
        exec_builder = exec_builder.push_deploy(deploy);
    }

    let exec_request = exec_builder.build();

    let builder = builder.exec(exec_request).expect_success();
    if should_commit {
        builder.commit();
    }
}

pub fn transfer_to_existing_accounts(group: &mut BenchmarkGroup<WallTime>, should_commit: bool) {
    let target_account = TARGET_ADDR;
    let bootstrap_accounts = vec![target_account];

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = bootstrap(data_dir.path(), bootstrap_accounts.clone(), U512::one());

    group.bench_function(
        format!(
            "transfer_to_existing_account_multiple_execs/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            b.iter(|| {
                // Execute multiple deploys with multiple exec requests
                transfer_to_account_multiple_execs(&mut builder, target_account, should_commit)
            })
        },
    );

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = bootstrap(data_dir.path(), bootstrap_accounts, U512::one());

    group.bench_function(
        format!(
            "transfer_to_existing_account_multiple_deploys_per_exec/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            b.iter(|| {
                // Execute multiple deploys with a single exec request
                transfer_to_account_multiple_deploys(&mut builder, target_account, should_commit)
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
    let mut builder = bootstrap(
        data_dir.path(),
        bootstrap_accounts,
        U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
    );

    let purse_amount = U512::one();
    let purses = create_purses(
        &mut builder,
        target_account,
        purse_count as u64,
        purse_amount,
    );

    let mut purse_index = 0usize;
    let mut exec_requests = Vec::with_capacity(transfer_count);
    for _ in 0..transfer_count {
        let account = {
            let account = purses[purse_index];
            if purse_index == purses.len() - 1 {
                purse_index = 0;
            } else {
                purse_index += 1;
            }
            account
        };
        let mut exec_builder = ExecuteRequestBuilder::new();
        let runtime_args = runtime_args! {
            ARG_TARGET => account,
            ARG_AMOUNT => U512::one(),
            ARG_ID => <Option<u64>>::None
        };
        let native_transfer = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! {})
            .with_transfer_args(runtime_args)
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .build();
        exec_builder = exec_builder.push_deploy(native_transfer);
        let exec_request = exec_builder.build();
        exec_requests.push(exec_request);
    }

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
                transfer_to_account_multiple_native_transfers(
                    &mut builder,
                    &exec_requests,
                    use_scratch,
                )
            })
        },
    );
}

/// This test simulates flushing at the end of a block.
fn transfer_to_account_multiple_native_transfers(
    builder: &mut LmdbWasmTestBuilder,
    execute_requests: &[ExecuteRequest],
    use_scratch: bool,
) {
    for exec_request in execute_requests {
        let request = ExecuteRequest::new(
            exec_request.parent_state_hash,
            exec_request.block_time,
            exec_request.deploys.clone(),
            exec_request.protocol_version,
            exec_request.proposer.clone(),
        );
        if use_scratch {
            builder.scratch_exec_and_commit(request).expect_success();
        } else {
            builder.exec(request).expect_success();
            builder.commit();
        }
    }
    if use_scratch {
        builder.write_scratch_to_lmdb();
    }
    // flush to disk only after entire block (simulates manual_sync_enabled=true config entry)
    builder.flush_environment();

    // WasmTestBuilder holds on to all execution results. This needs to be cleared to reduce
    // overhead in this test - it will likely OOM without.
    builder.clear_results();
}

pub fn transfer_to_existing_purses(group: &mut BenchmarkGroup<WallTime>, should_commit: bool) {
    let target_account = TARGET_ADDR;
    let bootstrap_accounts = vec![target_account];

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = bootstrap(
        data_dir.path(),
        bootstrap_accounts.clone(),
        U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) * 10,
    );

    let purse_amount = U512::one();
    let purses = create_purses(&mut builder, target_account, 1, purse_amount);

    group.bench_function(
        format!(
            "transfer_to_purse_multiple_execs/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            let target_purse = purses[0];
            b.iter(|| {
                // Execute multiple deploys with multiple exec request
                transfer_to_purse_multiple_execs(&mut builder, target_purse, should_commit)
            })
        },
    );

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = bootstrap(
        data_dir.path(),
        bootstrap_accounts,
        U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) * 10,
    );
    let purses = create_purses(&mut builder, TARGET_ADDR, 1, U512::one());

    group.bench_function(
        format!(
            "transfer_to_purse_multiple_deploys_per_exec/{}/{}",
            TRANSFER_BATCH_SIZE, should_commit
        ),
        |b| {
            let target_purse = purses[0];
            b.iter(|| {
                // Execute multiple deploys with a single exec request
                transfer_to_purse_multiple_deploys(&mut builder, target_purse, should_commit)
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
