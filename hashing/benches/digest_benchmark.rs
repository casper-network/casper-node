use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use hashing::Digest;

fn digest_benchmark(c: &mut Criterion) {
    let hashing_data = Arc::new(vec![0u8; 1_048_576 * 10]);
    c.bench_function("Digest::hash", move |b| {
        let hashing_data = Arc::clone(&hashing_data);
        b.iter(move || {
            let hashing_data = Arc::clone(&hashing_data);
            Digest::hash(&*hashing_data)
        });
    });
}

criterion_group!(benches, digest_benchmark);
criterion_main!(benches);
