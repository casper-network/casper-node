use std::time::{Duration, Instant};

use lmdb::{RwTransaction, Transaction};
use tracing::{info, warn};

use casper_execution_engine::{
    core::engine_state::EngineState,
    storage::{
        global_state::lmdb::LmdbGlobalState,
        transaction_source::{Readable, TransactionSource, Writable},
        trie::{Pointer, Trie},
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{self, Bytes, ToBytes},
    Key, StoredValue,
};

pub fn copy_state_root(
    state_root: Digest,
    source: &EngineState<LmdbGlobalState>,
    destination: &EngineState<LmdbGlobalState>,
) -> Result<(), anyhow::Error> {
    let mut missing_trie_keys = vec![state_root];
    let start_time = Instant::now();
    let mut heartbeat_interval = Instant::now();

    let mut total_tries: u64 = 0;
    let mut total_bytes: u64 = 0;

    let mut time_searching_for_trie_keys = Duration::from_secs(0);

    while let Some(next_trie_key) = missing_trie_keys.pop() {
        // For user feedback, update on progress if this takes longer than 10 seconds.
        if heartbeat_interval.elapsed().as_secs() > 10 {
            info!(
                "trie migration progress: bytes copied {}, tries copied {}",
                total_bytes, total_tries,
            );
            heartbeat_interval = Instant::now();
        }

        let source_store = source.get_state().trie_store();
        let destination_store = destination.get_state().trie_store();
        let trie_key_bytes = next_trie_key
            .to_bytes()
            .map_err(|err| anyhow::anyhow!("tobytes {:?}", err))?;

        let read_txn = source.get_state().environment().create_read_txn()?;
        let mut write_txn = destination
            .get_state()
            .environment()
            .create_read_write_txn()?;

        match read_txn.read(source_store.get_db(), &trie_key_bytes)? {
            Some(value_bytes) => {
                let key_bytes = next_trie_key
                    .to_bytes()
                    .map_err(|err| anyhow::anyhow!("tobytes {:?}", err))?;
                let read_bytes = key_bytes.len() as u64 + value_bytes.len() as u64;
                total_bytes += read_bytes;
                total_tries += 1;

                write_txn.write(destination_store.get_db(), &key_bytes, &value_bytes)?;

                memoized_find_missing_descendants(
                    value_bytes,
                    destination_store,
                    &write_txn,
                    &mut missing_trie_keys,
                    &mut time_searching_for_trie_keys,
                )?;
            }
            None => {
                return Err(anyhow::anyhow!(
                    "error migrating state root {} {}, ",
                    state_root,
                    next_trie_key
                ));
            }
        }
        read_txn.commit()?;
        write_txn.commit()?;
    }

    info!(
        %total_bytes,
        %total_tries,
        time_migration_took_micros = %start_time.elapsed().as_micros(),
        time_searching_for_trie_keys_micros = %time_searching_for_trie_keys.as_micros(),
        "trie migration complete",
    );
    Ok(())
}

fn memoized_find_missing_descendants<'env>(
    value_bytes: Bytes,
    trie_store: &LmdbTrieStore,
    txn: &RwTransaction<'env>,
    missing_trie_keys: &mut Vec<Digest>,
    time_in_missing_trie_keys: &mut Duration,
) -> Result<(), anyhow::Error> {
    // A first bytes of `0` indicates a leaf. We short-circuit the function here to speed things up.
    if let Some(0u8) = value_bytes.get(0) {
        return Ok(());
    }
    let start_trie_keys = Instant::now();
    let trie: Trie<Key, StoredValue> = bytesrepr::deserialize(value_bytes.into())
        .map_err(|err| anyhow::anyhow!("deserialize failed {:?}", err))?;
    match trie {
        Trie::Leaf { .. } => {
            // If `bytesrepr` is functioning correctly, this should never be reached (see
            // optimization above), but it is still correct do nothing here.
            warn!("did not expect to see a trie leaf in `find_missing_descendents` after shortcut");
        }
        Trie::Node { pointer_block } => {
            for (_index, ptr) in pointer_block.as_indexed_pointers() {
                find_missing_trie_keys(ptr, missing_trie_keys, trie_store, txn)?;
            }
        }
        Trie::Extension { affix: _, pointer } => {
            find_missing_trie_keys(pointer, missing_trie_keys, trie_store, txn)?;
        }
    }
    *time_in_missing_trie_keys += start_trie_keys.elapsed();
    Ok(())
}

fn find_missing_trie_keys<'env>(
    ptr: Pointer,
    missing_trie_keys: &mut Vec<Digest>,
    handle: &LmdbTrieStore,
    txn: &RwTransaction<'env>,
) -> Result<(), anyhow::Error> {
    let ptr = match ptr {
        Pointer::LeafPointer(pointer) | Pointer::NodePointer(pointer) => pointer,
    };
    let existing = txn.read(
        handle.get_db(),
        &ptr.to_bytes()
            .map_err(|err| anyhow::anyhow!("tobytes {:?}", err))?,
    )?;
    if existing.is_none() {
        missing_trie_keys.push(ptr);
    }
    Ok(())
}
