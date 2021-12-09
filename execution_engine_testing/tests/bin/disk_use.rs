use tempfile::TempDir;

use casper_engine_test_support::{
    auction::{self},
    transfer, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::U512;

// Generate multiple purses as well as transfer requests between them with the specified count.
pub fn multiple_native_transfers(
    transfer_count: usize,
    purse_count: usize,
    use_scratch: bool,
    run_auction: bool,
) {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
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
    println!("on_disk_size_bytes, transfer_count",);

    // 5 eras, of 30 blocks each, with `transfer_count` transfers.
    for _ in 0..5 {
        for _ in 0..30 {
            total_transfers += exec_requests.len();
            transfer::transfer_to_account_multiple_native_transfers(
                &mut builder,
                &exec_requests,
                use_scratch,
            );
            println!(
                "{}, {}",
                builder.lmdb_on_disk_size().unwrap(),
                total_transfers
            );
        }
        if run_auction {
            println!("running auction");
            auction::step_and_run_auction(&mut builder, &validator_keys);
        }
    }
}

fn main() {
    for purse_count in [100] {
        for transfer_count in [2500usize] {
            // baseline, one deploy per exec request
            multiple_native_transfers(transfer_count, purse_count, false, false);
            multiple_native_transfers(transfer_count, purse_count, true, false);
        }
    }
}
