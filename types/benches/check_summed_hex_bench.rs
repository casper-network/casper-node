use casper_types::check_summed_hex::{encode, encode_iter};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::Rng;

fn encode_vs_encode_iter(c: &mut Criterion) {
    let input: Vec<u8> = (0..100_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("encode_vs_encode_iter");

    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("encode_iter", |b| b.iter(|| encode_iter(black_box(&input))));
    group.finish();
}

fn encode_vs_encode_iter_first_5_chars(c: &mut Criterion) {
    let input: Vec<u8> = (0..100_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("encode_vs_encode_iter_first_5");

    // group.bench
}

criterion_group!(benches, encode_vs_encode_iter);
criterion_main!(benches);
