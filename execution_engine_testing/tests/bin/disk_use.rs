use std::{collections::HashMap, error::Error};

use casper_execution_engine::storage::trie::Trie;
use casper_hashing::Digest;
use lmdb::{Cursor, Transaction};
use tempfile::TempDir;

use casper_engine_test_support::{
    auction::{self},
    transfer, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::{account::AccountHash, bytesrepr::FromBytes, Key, StoredValue, U512};

const TARGET_ADDR: AccountHash = AccountHash::new([127; 32]);

fn transfer_disk_use(transfer_count: usize, purse_count: usize) -> Result<(), Box<dyn Error>> {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());

    let purse_amount = U512::one();
    transfer::create_initial_accounts_and_run_genesis(
        &mut builder,
        vec![TARGET_ADDR],
        purse_amount,
    );

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

    total_transfers += exec_requests.len();
    transfer::transfer_to_account_multiple_native_transfers(&mut builder, &exec_requests, true);
    println!(
        "{}, {}",
        builder.lmdb_on_disk_size().unwrap(),
        total_transfers
    );

    let env = builder.lmdb_environment();
    let db = builder.lmdb_handle();
    let env = env.env();

    let txn = env.begin_ro_txn()?;
    let mut cursor = txn.open_ro_cursor(db)?;
    let mut record_count = 0;
    let mut largest_record = 0;

    let mut key_tags = HashMap::<String, usize>::new();
    let mut stored_value_tags = HashMap::<String, usize>::new();
    let mut trie_value_lengths = HashMap::<(String, String), Vec<usize>>::new();
    let mut pointer_block_lengths = Vec::new();
    let mut extension_node_lengths = Vec::new();
    let mut total_value_bytes_read = 0;
    let mut total_key_bytes_read = 0;

    for (key, value) in cursor.iter() {
        // count key len
        total_key_bytes_read += key.len();

        let (_key, _rest) = Digest::from_bytes(key).expect("should deserialize");
        let byte_len = value.len();

        // count value len
        total_value_bytes_read += byte_len;

        let (trie, _remainder) =
            Trie::<Key, StoredValue>::from_bytes(value).expect("should be a trie");

        match trie {
            Trie::Leaf { key, value } => {
                let key_tag = key.type_string();
                let stored_value_tag = value.type_name();

                *key_tags.entry(key_tag.clone()).or_default() += 1;
                *stored_value_tags
                    .entry(stored_value_tag.clone())
                    .or_default() += 1;
                let trie_length_values = trie_value_lengths
                    .entry((key_tag, stored_value_tag))
                    .or_default();
                trie_length_values.push(byte_len);
            }
            Trie::Node { pointer_block: _ } => {
                pointer_block_lengths.push(byte_len);
            }
            Trie::Extension {
                affix: _,
                pointer: _,
            } => extension_node_lengths.push(byte_len),
        }

        record_count += 1;
        let serialized_len = value.len();
        if largest_record < serialized_len {
            println!("found new largest DB entry with len {}", serialized_len);
            largest_record = serialized_len;
        }
    }

    println!("key_tag, count");
    for (key_tag, count) in key_tags {
        println!("\"{}\", {}", key_tag, count);
    }

    println!("stored_value_tag, count");
    for (stored_value_tag, count) in stored_value_tags {
        println!("\"{}\", {}", stored_value_tag, count);
    }

    println!("key_tag, stored_value_tag, average_len, max_len, count, sum_total_bytes");
    for ((key_tag, stored_value_tag), lengths) in trie_value_lengths {
        if lengths.is_empty() {
            continue;
        }
        let total: usize = lengths.iter().sum::<usize>();
        let average_len: usize = total / lengths.len();
        let max_len: usize = *lengths.iter().max().unwrap();
        println!(
            "\"{}\", \"{}\", {}, {}, {}, {}",
            key_tag,
            stored_value_tag,
            average_len,
            max_len,
            lengths.len(),
            total,
        );
    }

    println!(
        "processed {} db records total, {} key bytes read, {} value bytes read, {} total bytes read, {} lmdb file size in bytes",
        record_count,
        total_key_bytes_read,
        total_value_bytes_read,
        total_value_bytes_read + total_key_bytes_read,
        builder.lmdb_on_disk_size().unwrap()
    );
    println!(
        "{} pointer blocks consuming {} bytes",
        pointer_block_lengths.len(),
        pointer_block_lengths.iter().sum::<usize>()
    );
    println!(
        "{} extension nodes consuming {} bytes",
        extension_node_lengths.len(),
        extension_node_lengths.iter().sum::<usize>()
    );

    Ok(())
}

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
    transfer_disk_use(2500, 100).unwrap();
    for purse_count in [100] {
        for transfer_count in [2500usize] {
            // baseline, one deploy per exec request
            multiple_native_transfers(transfer_count, purse_count, false, false);
            multiple_native_transfers(transfer_count, purse_count, true, false);
        }
    }
}
