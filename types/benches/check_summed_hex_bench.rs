use base16;
use casper_types::check_summed_hex::{decode, encode, encode_iter};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn encode_and_print_first_5(input: &[u8]) {
    let encoded = encode(input);
    format!("{:5}", encoded);
}

fn encode_iter_and_print_first_5(input: &[u8]) {
    let encoded = encode_iter(input).take(5).collect::<String>();
    format!("{:5}", encoded);
}

fn encode_iter_collected(input: &[u8]) -> String {
    encode_iter(input).collect()
}

fn encode_vs_encode_iter(c: &mut Criterion) {
    let input: Vec<u8> = (0..100_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("encode_vs_encode_iter");

    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("encode_iter_collected", |b| {
        b.iter(|| encode_iter_collected(black_box(&input)))
    });
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

fn encode_vs_base16_encode_lower_10kb(c: &mut Criterion) {
    let input: Vec<u8> = (0..10_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("encode_vs_encode_lower");
    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("base16_encode_lower", |b| {
        b.iter(|| base16::encode_lower(black_box(&input)))
    });
    group.finish();
}

fn encode_vs_base16_encode_lower_100kb(c: &mut Criterion) {
    let input: Vec<u8> = (0..100_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("encode_vs_encode_lower");
    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("base16_encode_lower", |b| {
        b.iter(|| base16::encode_lower(black_box(&input)))
    });
    group.finish();
}

fn encode_vs_base16_encode_lower_1mb(c: &mut Criterion) {
    let input: Vec<u8> = (0..1_000_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("encode_vs_encode_lower");
    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("base16_encode_lower", |b| {
        b.iter(|| base16::encode_lower(black_box(&input)))
    });
    group.finish();
}

fn decode_vs_base16_decode_checksummed_hex(c: &mut Criterion) {
    let input: Vec<u8> = (0..10_000).map(|_| rand::random::<u8>()).collect();
    let input = encode(&input);
    let mut group = c.benchmark_group("decode_checksummed_hex");
    group.bench_function("decode", |b| b.iter(|| decode(black_box(&input))));
    group.bench_function("base16_decode", |b| {
        b.iter(|| base16::decode(black_box(&input)))
    });
    group.finish();
}

fn decode_vs_base16_decode_unchecked_hex(c: &mut Criterion) {
    let input: Vec<u8> = (0..10_000).map(|_| rand::random::<u8>()).collect();
    let input = base16::encode_lower(&input);
    let mut group = c.benchmark_group("decode_unchecked_hex");

    group.bench_function("decode", |b| b.iter(|| decode(black_box(&input))));
    group.bench_function("base16_decode", |b| {
        b.iter(|| base16::decode(black_box(&input)))
    });
    group.finish();
}

criterion_group!(
    benches,
    encode_vs_encode_iter,
    encode_vs_encode_iter_first_5_chars,
    encode_vs_base16_encode_lower_10kb,
    encode_vs_base16_encode_lower_100kb,
    encode_vs_base16_encode_lower_1mb,
    decode_vs_base16_decode_checksummed_hex,
    decode_vs_base16_decode_unchecked_hex
);
criterion_main!(benches);
