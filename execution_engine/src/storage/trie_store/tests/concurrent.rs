use std::{
    sync::{Arc, Barrier},
    thread,
};

use casper_types::bytesrepr::Bytes;
use tempfile::tempdir;

use super::TestData;
use crate::storage::{
    store::Store,
    transaction_source::{db::RocksDbStore, rocksdb_defaults},
    trie::Trie,
    trie_store::in_memory::InMemoryTrieStore,
};

#[test]
fn lmdb_writer_mutex_does_not_collide_with_readers() {
    let dir = tempdir().unwrap();
    let store = RocksDbStore::new(dir.path(), rocksdb_defaults()).unwrap();
    let num_threads = 10;
    let barrier = Arc::new(Barrier::new(num_threads + 1));
    let mut handles = Vec::new();
    let TestData(ref leaf_1_hash, ref leaf_1) = &super::create_data()[0..1][0];

    for _ in 0..num_threads {
        let reader_store = store.clone();
        let reader_barrier = barrier.clone();
        let leaf_1_hash = *leaf_1_hash;
        #[allow(clippy::clone_on_copy)]
        let leaf_1 = leaf_1.clone();

        handles.push(thread::spawn(move || {
            {
                let result: Option<Trie<Bytes, Bytes>> = reader_store.get(&leaf_1_hash).unwrap();
                assert_eq!(result, None);
            }
            // wait for other reader threads to read and the main thread to
            // take a read-write transaction
            reader_barrier.wait();
            // wait for main thread to put and commit
            reader_barrier.wait();
            {
                let result: Option<Trie<Bytes, Bytes>> = reader_store.get(&leaf_1_hash).unwrap();
                result.unwrap() == leaf_1
            }
        }));
    }

    // wait for reader threads to read
    barrier.wait();
    store.put(leaf_1_hash, leaf_1).unwrap();
    // sync with reader threads
    barrier.wait();

    assert!(handles.into_iter().all(|b| b.join().unwrap()))
}

#[test]
fn in_memory_writer_mutex_does_not_collide_with_readers() {
    let store = Arc::new(InMemoryTrieStore::new());
    let num_threads = 10;
    let barrier = Arc::new(Barrier::new(num_threads + 1));
    let mut handles = Vec::new();
    let TestData(ref leaf_1_hash, ref leaf_1) = &super::create_data()[0..1][0];

    for _ in 0..num_threads {
        let reader_store = store.clone();
        let reader_barrier = barrier.clone();
        let leaf_1_hash = *leaf_1_hash;
        #[allow(clippy::clone_on_copy)]
        let leaf_1 = leaf_1.clone();

        handles.push(thread::spawn(move || {
            {
                let result: Option<Trie<Bytes, Bytes>> = reader_store.get(&leaf_1_hash).unwrap();
                assert_eq!(result, None);
            }
            // wait for other reader threads to read and the main thread to
            // take a read-write transaction
            reader_barrier.wait();
            // wait for main thread to put and commit
            reader_barrier.wait();
            {
                let result: Option<Trie<Bytes, Bytes>> = reader_store.get(&leaf_1_hash).unwrap();
                result.unwrap() == leaf_1
            }
        }));
    }

    // wait for reader threads to read
    barrier.wait();
    store.put(leaf_1_hash, leaf_1).unwrap();
    // sync with reader threads
    barrier.wait();

    assert!(handles.into_iter().all(|b| b.join().unwrap()))
}
