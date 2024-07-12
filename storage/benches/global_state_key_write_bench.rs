use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use pprof::criterion::{Output, PProfProfiler};

use casper_storage::global_state::{
    error,
    store::Store,
    transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
    trie::Trie,
    trie_store::{lmdb::LmdbTrieStore, operations::WriteResult},
};
use casper_types::{bytesrepr::ToBytes, testing::TestRng, Digest, Key};
use lmdb::{DatabaseFlags, RwTransaction};
use rand::Rng;
use tempfile::tempdir;

use casper_storage::global_state::trie_store::operations::write;

pub(crate) const DB_SIZE: usize = 8_520_428_800;
pub(crate) const MAX_READERS: u32 = 512;

fn write_batch(
    trie_store: &LmdbTrieStore,
    txn: &mut RwTransaction,
    mut root_hash: Digest,
    data: Vec<(Key, u32)>,
) {
    for (key, value) in data.iter() {
        let write_result =
            write::<Key, u32, _, _, error::Error>(txn, trie_store, &root_hash, key, value).unwrap();
        match write_result {
            WriteResult::Written(hash) => {
                root_hash = hash;
            }
            WriteResult::AlreadyExists => (),
            WriteResult::RootNotFound => panic!("write_leaves given an invalid root"),
        };
    }
}

fn empty_store() -> (LmdbEnvironment, LmdbTrieStore, Digest) {
    let _temp_dir = tempdir().unwrap();
    let environment = LmdbEnvironment::new(_temp_dir.path(), DB_SIZE, MAX_READERS, true).unwrap();
    let store = LmdbTrieStore::new(&environment, None, DatabaseFlags::empty()).unwrap();

    let trie: Trie<Key, u32> = Trie::node(&[]);
    let trie_bytes = trie.to_bytes().unwrap();
    let hash = Digest::hash(trie_bytes);

    let mut txn = environment.create_read_write_txn().unwrap();
    store.put(&mut txn, &hash, &trie).unwrap();
    txn.commit().unwrap();

    (environment, store, hash)
}

fn custom_iteration_write_bench(c: &mut Criterion) {
    let mut rng = TestRng::new();

    for batch_size in [1000, 10_000] {
        //[1000u32, 10_000, 100_000, 250_000] {
        let mut group = c.benchmark_group(format!("trie_store_batch_write_{}", batch_size));
        group.throughput(Throughput::Elements(batch_size as u64));

        if batch_size > 150_000 {
            // Reduce the sample size to allow faster runtime.
            group.sample_size(30);
        }

        group.bench_function("write_batch", |b| {
            b.iter_custom(|iter| {
                let mut total = Duration::default();
                for _ in 0..iter {
                    let (env, store, root_hash) = empty_store();
                    let mut txn = env.create_read_write_txn().unwrap();
                    let data: Vec<(Key, u32)> = (0u32..batch_size)
                        .into_iter()
                        .map(|val| (rng.gen(), val))
                        .collect();

                    let start = Instant::now();
                    write_batch(&store, &mut txn, root_hash, data);
                    total = total.checked_add(start.elapsed()).unwrap();
                }

                total
            })
        });
        group.finish();
    }
}

criterion_group! {
  name = benches;
  config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
  targets = custom_iteration_write_bench
}
criterion_main!(benches);
