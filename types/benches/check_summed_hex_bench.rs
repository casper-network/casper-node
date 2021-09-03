use casper_types::check_summed_hex::{encode, encode_iter};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn encode_and_print_first_5(input: &[u8]) {
    let encoded = encode(input);
    format!("{:5}", encoded);
}

fn encode_iter_and_print_first_5(input: &[u8]) {
    let encoded = encode_iter(input).take(5).collect::<String>();
    format!("{:5}", encoded);
}

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
    group.bench_function("encode_first_5", |b| {
        b.iter(|| encode_and_print_first_5(black_box(&input)))
    });
    group.bench_function("encode_iter_first_5", |b| {
        b.iter(|| encode_iter_and_print_first_5(black_box(&input)))
    });
    group.finish();
}

criterion_group!(
    benches,
    encode_vs_encode_iter,
    encode_vs_encode_iter_first_5_chars
);
criterion_main!(benches);
