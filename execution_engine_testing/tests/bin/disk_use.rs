use std::{fs::File, io::BufWriter};

use casper_engine_test_support::auction::run_blocks_with_transfers_and_step;

fn main() {
    let purse_count = 100;
    let total_transfer_count = 100;
    let transfers_per_block = 1;
    let block_count = total_transfer_count / transfers_per_block;
    let delegator_count = 20_000;
    let validator_count = 100;

    let report_writer = BufWriter::new(File::create("disk_use_report.csv").unwrap());
    run_blocks_with_transfers_and_step(
        transfers_per_block,
        purse_count,
        true,
        true,
        block_count,
        delegator_count,
        validator_count,
        report_writer,
    );
}
