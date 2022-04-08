use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

use tempfile::TempDir;

use casper_engine_test_support::{
    auction::{self},
    transfer, DbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::U512;

// Generate multiple purses as well as transfer requests between them with the specified count.
pub fn multiple_native_transfers(
    transfer_count: usize,
    purse_count: usize,
    use_scratch: bool,
    run_auction: bool,
    block_count: usize,
) -> Vec<(usize, usize, usize, usize)> {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = DbWasmTestBuilder::new(data_dir.as_ref());
    let delegator_keys = auction::generate_public_keys(100);
    let validator_keys = auction::generate_public_keys(100);

    auction::run_genesis_and_create_initial_accounts(
        &mut builder,
        &validator_keys,
        delegator_keys
            .iter()
            .map(|pk| pk.to_account_hash())
            .collect::<Vec<_>>(),
        U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
    );
    let contract_hash = builder.get_auction_contract_hash();
    let mut next_validator_iter = validator_keys.iter().cycle();
    for delegator_public_key in delegator_keys {
        let delegation_amount = U512::from(42);
        let delegator_account_hash = delegator_public_key.to_account_hash();
        let next_validator_key = next_validator_iter
            .next()
            .expect("should produce values forever");
        let delegate = auction::create_delegate_request(
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

    let purse_amount = U512::one();

    let purses = transfer::create_test_purses(
        &mut builder,
        *DEFAULT_ACCOUNT_ADDR,
        purse_count as u64,
        purse_amount,
    );

    let exec_requests = transfer::create_multiple_native_transfers_to_purses(
        *DEFAULT_ACCOUNT_ADDR,
        transfer_count,
        &purses,
    );

    let mut total_transfers = 0;

    let mut results = Vec::new();
    // simulating a block boundary here.
    for _ in 0..block_count {
        let start = Instant::now();
        total_transfers += exec_requests.len();
        transfer::transfer_to_account_multiple_native_transfers(
            &mut builder,
            &exec_requests,
            use_scratch,
        );
        let exec_time = start.elapsed();
        results.push((
            builder.lmdb_on_disk_size().unwrap() as usize,
            builder.rocksdb_on_disk_size().unwrap(),
            total_transfers,
            exec_time.as_millis() as usize,
        ));
        println!(
            "{}, {}, {}, transfer, {}",
            builder.lmdb_on_disk_size().unwrap(),
            builder.rocksdb_on_disk_size().unwrap(),
            total_transfers,
            exec_time.as_millis(),
        );
        let threshold = Duration::from_millis((2500 / 500) * exec_requests.len() as u64);
        if exec_time >= threshold {
            println!("{:?}", Instant::now());
        }
    }
    if run_auction {
        let start = Instant::now();
        auction::step_and_run_auction(&mut builder, &validator_keys);
        println!(
            "{}, {}, {}, ran_auction, {}",
            builder.lmdb_on_disk_size().unwrap(),
            builder.rocksdb_on_disk_size().unwrap(),
            total_transfers,
            start.elapsed().as_millis(),
        );
    }
    results
}

fn main() {
    let purse_count = 100;
    let total_transfer_count = 1_000_000;
    let transfers_per_block = 2500;
    let block_count = total_transfer_count / transfers_per_block;

    let mut results = Vec::new();

    let column_name = format!("rocksdb-{}", transfers_per_block);
    let mut bytes_report =
        BufWriter::new(File::create(format!("bytes-report-{}.csv", column_name)).unwrap());
    let mut time_report =
        BufWriter::new(File::create(format!("time-report-{}.csv", column_name)).unwrap());

    let rocksdb_results =
        multiple_native_transfers(transfers_per_block, purse_count, true, false, block_count);
    let transfer_counts = rocksdb_results
        .iter()
        .map(|(_, _, transfers, _)| *transfers)
        .collect::<Vec<_>>();
    results.push((column_name, rocksdb_results));

    // add columns for additional reports by pushing onto `results` here.

    let mut byte_size_map = HashMap::new();
    let mut time_ms_map = HashMap::new();

    for (column, values) in results.iter() {
        let bytes_values: Vec<usize> = values
            .iter()
            .map(|(_, byte_size, _, _)| *byte_size)
            .collect();
        byte_size_map.insert(column.clone(), bytes_values);
        let time_ms_values: Vec<usize> = values.iter().map(|(_, _, _, time_ms)| *time_ms).collect();
        time_ms_map.insert(column.clone(), time_ms_values);
    }

    // bytes
    println!("bytes report");
    for (column, _) in results.iter() {
        write!(&mut bytes_report, "{}-bytes,", column).unwrap();
    }
    writeln!(&mut bytes_report, "transfers").unwrap();

    for (idx, transfer_count) in transfer_counts.iter().enumerate() {
        for (column, _) in results.iter() {
            write!(&mut bytes_report, " {},", byte_size_map[column][idx]).unwrap();
        }
        writeln!(&mut bytes_report, " {}", transfer_count).unwrap();
    }

    println!("time_ms report");
    //
    // Time ms
    for (column, _) in results.iter() {
        write!(&mut time_report, "{}-time_ms,", column).unwrap();
    }
    writeln!(&mut time_report, "transfers").unwrap();
    for (idx, transfer_count) in transfer_counts.iter().enumerate() {
        for (column, _) in results.iter() {
            write!(&mut time_report, " {},", time_ms_map[column][idx]).unwrap();
        }
        writeln!(&mut time_report, " {}", transfer_count).unwrap();
    }

    println!("wrote reports");
}
