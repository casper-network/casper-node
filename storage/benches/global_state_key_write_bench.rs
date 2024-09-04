use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use pprof::criterion::{Output, PProfProfiler};

use casper_storage::global_state::{
    error,
    store::Store,
    transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
    trie::Trie,
    trie_store::{
        lmdb::LmdbTrieStore,
        operations::{batch_write, WriteResult},
    },
};
use casper_types::{bytesrepr::ToBytes, testing::TestRng, Digest, Key};
use lmdb::{DatabaseFlags, RwTransaction};
use rand::Rng;
use tempfile::tempdir;

use casper_storage::global_state::trie_store::operations::write;

pub(crate) const DB_SIZE: usize = 8_520_428_800;
pub(crate) const MAX_READERS: u32 = 512;

fn write_sequential(
    trie_store: &LmdbTrieStore,
    txn: &mut RwTransaction,
    mut root_hash: Digest,
    data: Vec<(Key, u32)>,
) -> Digest {
    for (key, value) in data.iter() {
        let write_result =
            write::<Key, u32, _, _, error::Error>(txn, trie_store, &root_hash, key, value).unwrap();
        match write_result {
            WriteResult::Written(hash) => {
                root_hash = hash;
            }
            WriteResult::AlreadyExists => (),
            WriteResult::RootNotFound => panic!("invalid root hash"),
        };
    }
    root_hash
}

fn create_empty_store() -> (LmdbEnvironment, LmdbTrieStore) {
    let _temp_dir = tempdir().unwrap();
    let environment = LmdbEnvironment::new(_temp_dir.path(), DB_SIZE, MAX_READERS, true).unwrap();
    let store = LmdbTrieStore::new(&environment, None, DatabaseFlags::empty()).unwrap();

    (environment, store)
}

fn store_empty_root(env: &LmdbEnvironment, store: &LmdbTrieStore) -> Digest {
    let trie: Trie<Key, u32> = Trie::node(&[]);
    let trie_bytes = trie.to_bytes().unwrap();
    let hash = Digest::hash(trie_bytes);

    let mut txn = env.create_read_write_txn().unwrap();
    store.put(&mut txn, &hash, &trie).unwrap();
    txn.commit().unwrap();

    hash
}

fn sequential_write_bench(c: &mut Criterion, rng: &mut TestRng) {
    let mut sequential_write_group = c.benchmark_group("trie_store_sequential_write");
    for batch_size in [1000, 10_000] {
        sequential_write_group.throughput(Throughput::Elements(batch_size as u64));

        if batch_size > 150_000 {
            // Reduce the sample size to allow faster runtime.
            sequential_write_group.sample_size(30);
        }

        sequential_write_group.bench_function(format!("write_sequential_{}", batch_size), |b| {
            b.iter_custom(|iter| {
                let mut total = Duration::default();
                for _ in 0..iter {
                    let (env, store) = create_empty_store();
                    let root_hash = store_empty_root(&env, &store);
                    let mut txn = env.create_read_write_txn().unwrap();
                    let data: Vec<(Key, u32)> =
                        (0u32..batch_size).map(|val| (rng.gen(), val)).collect();

                    let start = Instant::now();
                    write_sequential(&store, &mut txn, root_hash, data);
                    total = total.checked_add(start.elapsed()).unwrap();
                }

                total
            })
        });
    }
    sequential_write_group.finish();
}

fn batch_write_with_empty_store(c: &mut Criterion, rng: &mut TestRng) {
    let mut batch_write_group = c.benchmark_group("batch_write_with_empty_store");

    for batch_size in [1000, 10_000] {
        batch_write_group.throughput(Throughput::Elements(batch_size as u64));

        if batch_size > 150_000 {
            // Reduce the sample size to allow faster runtime.
            batch_write_group.sample_size(30);
        }

        batch_write_group.bench_function(format!("write_batch_{}", batch_size), |b| {
            b.iter_custom(|iter| {
                let mut total = Duration::default();
                for _ in 0..iter {
                    let (environment, store) = create_empty_store();
                    let root_hash = store_empty_root(&environment, &store);
                    let mut txn = environment.create_read_write_txn().unwrap();
                    let data: Vec<(Key, u32)> =
                        (0u32..batch_size).map(|val| (rng.gen(), val)).collect();

                    let start = Instant::now();
                    let _ = batch_write::<Key, u32, _, _, _, error::Error>(
                        &mut txn,
                        &store,
                        &root_hash,
                        data.into_iter(),
                    )
                    .unwrap();
                    total = total.checked_add(start.elapsed()).unwrap();
                }

                total
            })
        });
    }
    batch_write_group.finish();
}

fn batch_write_with_populated_store(c: &mut Criterion, rng: &mut TestRng) {
    let mut batch_write_group = c.benchmark_group("batch_write_with_populated_store");

    for batch_size in [1000, 10_000] {
        batch_write_group.throughput(Throughput::Elements(batch_size as u64));

        if batch_size > 150_000 {
            // Reduce the sample size to allow faster runtime.
            batch_write_group.sample_size(30);
        }

        batch_write_group.bench_function(format!("write_batch_{}", batch_size), |b| {
            b.iter_custom(|iter| {
                let mut total = Duration::default();
                for _ in 0..iter {
                    let (environment, store) = create_empty_store();
                    let root_hash = store_empty_root(&environment, &store);
                    let mut txn = environment.create_read_write_txn().unwrap();
                    let initial_data: Vec<(Key, u32)> =
                        (0u32..200).map(|val| (rng.gen(), val)).collect();

                    // Pre-populate trie store with some data.
                    let root_hash = write_sequential(&store, &mut txn, root_hash, initial_data);

                    // Create a cache backed up by the pre-populated store. Any already existing
                    // nodes will be read-back into the cache.
                    let data: Vec<(Key, u32)> =
                        (0u32..batch_size).map(|val| (rng.gen(), val)).collect();

                    let start = Instant::now();
                    let _ = batch_write::<Key, u32, _, _, _, error::Error>(
                        &mut txn,
                        &store,
                        &root_hash,
                        data.into_iter(),
                    )
                    .unwrap();
                    total = total.checked_add(start.elapsed()).unwrap();
                }

                total
            })
        });
    }
    batch_write_group.finish();
}

fn trie_store_batch_write_bench(c: &mut Criterion) {
    let mut rng = TestRng::new();

    sequential_write_bench(c, &mut rng);
    batch_write_with_empty_store(c, &mut rng);
    batch_write_with_populated_store(c, &mut rng);
}

criterion_group! {
  name = benches;
  config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
  targets = trie_store_batch_write_bench
}
criterion_main!(benches);
