use std::{
    collections::HashSet,
    convert::TryFrom,
    fs::File,
    io::{BufWriter, Write},
    time::Instant,
};

use tempfile::TempDir;

use casper_engine_test_support::{
    auction::{self},
    transfer, DbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::{
    core::{
        engine_state::{self, EngineState},
        execution,
    },
    shared::newtypes::CorrelationId,
    storage::{
        global_state::{CommitProvider, StateProvider},
        trie::{Pointer, Trie, TrieOrChunk, TrieOrChunkId},
    },
};
use casper_hashing::Digest;
use casper_types::{bytesrepr, Key, StoredValue, U512};

const TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE: u64 = 1_000_000 * 1_000_000_000;

// Generate multiple purses as well as transfer requests between them with the specified count.
pub fn multiple_native_transfers(
    transfer_count: usize,
    purse_count: usize,
    use_scratch: bool,
    run_auction: bool,
    block_count: usize,
    delegator_count: usize,
    mut report_writer: BufWriter<File>,
) {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = DbWasmTestBuilder::new(data_dir.as_ref());
    let delegator_keys = auction::generate_public_keys(delegator_count);
    let validator_keys = auction::generate_public_keys(100);
    let mut necessary_tries = HashSet::new();

    println!("creating genesis accounts");
    auction::run_genesis_and_create_initial_accounts(
        &mut builder,
        &validator_keys,
        delegator_keys
            .iter()
            .map(|pk| pk.to_account_hash())
            .collect::<Vec<_>>(),
        U512::from(TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE),
    );
    let contract_hash = builder.get_auction_contract_hash();
    let mut next_validator_iter = validator_keys.iter().cycle();

    println!("creating delegators");
    for delegator_public_key in delegator_keys {
        let delegation_amount = U512::from(2000 * 1_000_000_000u64);
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

    let purse_amount = U512::from(1_000_000_000);

    println!("creating test purses");
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

    println!("getting all keys up to this point");
    let mut total_transfers = 0;
    {
        let engine_state = builder.get_engine_state();
        let rocksdb = engine_state.get_state().rocksdb_state();

        let existing_keys = rocksdb
            .rocksdb
            .trie_store_iterator()
            .expect("unable to get iterator")
            .map(|(key, _)| Digest::try_from(&*key).expect("should be a digest"));
        necessary_tries.extend(existing_keys);
    }

    println!("found {} keys so far", necessary_tries.len());

    writeln!(
        report_writer,
        "height,rocksdb-size,transfers,time_ms,necessary_tries,total_tries"
    )
    .unwrap();
    // simulating a block boundary here.
    for current_block in 0..block_count {
        println!("executing height {current_block}");
        let start = Instant::now();
        total_transfers += exec_requests.len();
        transfer::transfer_to_account_multiple_native_transfers(
            &mut builder,
            &exec_requests,
            use_scratch,
        );
        let transfer_root = builder.get_post_state_hash();
        let maybe_auction_root = if run_auction {
            auction::step_and_run_auction(&mut builder, &validator_keys);
            Some(builder.get_post_state_hash())
        } else {
            None
        };
        let exec_time = start.elapsed();
        find_necessary_tries(
            builder.get_engine_state(),
            &mut necessary_tries,
            transfer_root,
        );

        if let Some(auction_root) = maybe_auction_root {
            find_necessary_tries(
                builder.get_engine_state(),
                &mut necessary_tries,
                auction_root,
            );
        }

        let engine_state = builder.get_engine_state();
        let rocksdb = engine_state.get_state().rocksdb_state();
        let trie_store_iter = rocksdb
            .rocksdb
            .trie_store_iterator()
            .expect("unable to get iterator");
        let total_tries = trie_store_iter.count();

        writeln!(
            report_writer,
            "{},{},{},{},{},{}",
            current_block,
            builder.rocksdb_on_disk_size().unwrap(),
            total_transfers,
            exec_time.as_millis() as usize,
            necessary_tries.len(),
            total_tries,
        )
        .unwrap();
        report_writer.flush().unwrap();
    }
}

// find all necessary tries - hoist to FN
fn find_necessary_tries<S>(
    engine_state: &EngineState<S>,
    necessary_tries: &mut HashSet<Digest>,
    state_root: Digest,
) where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
    engine_state::Error: From<S::Error>,
{
    let mut queue = Vec::new();
    queue.push(state_root);

    while let Some(root) = queue.pop() {
        if necessary_tries.contains(&root) {
            continue;
        }
        necessary_tries.insert(root);

        let trie_or_chunk: TrieOrChunk = engine_state
            .get_trie(CorrelationId::new(), TrieOrChunkId(0, root))
            .unwrap()
            .expect("trie should exist");

        let trie_bytes = match trie_or_chunk {
            TrieOrChunk::Trie(trie) => trie,
            TrieOrChunk::ChunkWithProof(_) => continue,
        };

        if let Some(0) = trie_bytes.get(0) {
            continue;
        }

        let trie: Trie<Key, StoredValue> =
            bytesrepr::deserialize(trie_bytes.inner_bytes().to_owned())
                .expect("unable to deserialize");

        match trie {
            Trie::Leaf { .. } => continue,
            Trie::Node { pointer_block } => queue.extend(pointer_block.as_indexed_pointers().map(
                |(_idx, ptr)| match ptr {
                    Pointer::LeafPointer(digest) | Pointer::NodePointer(digest) => digest,
                },
            )),
            Trie::Extension { affix: _, pointer } => match pointer {
                Pointer::LeafPointer(digest) | Pointer::NodePointer(digest) => queue.push(digest),
            },
        }
    }
}

fn main() {
    let purse_count = 100;
    let total_transfer_count = 100;
    let transfers_per_block = 1;
    let block_count = total_transfer_count / transfers_per_block;
    let delegator_count = 20_000;

    let report_writer = BufWriter::new(File::create("disk_use_report.csv").unwrap());
    multiple_native_transfers(
        transfers_per_block,
        purse_count,
        true,
        true,
        block_count,
        delegator_count,
        report_writer,
    );
}
