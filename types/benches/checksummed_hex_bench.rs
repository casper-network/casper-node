use criterion::{black_box, criterion_group, criterion_main, Criterion};

use casper_types::checksummed_hex::{decode, encode, SMALL_BYTES_COUNT};

fn encode_vs_base16_encode_lower_small(c: &mut Criterion) {
    let input: Vec<u8> = (0..SMALL_BYTES_COUNT)
        .map(|_| rand::random::<u8>())
        .collect();
    let mut group = c.benchmark_group("checksummed_hex_encode_small");
    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("base16_encode_lower", |b| {
        b.iter(|| base16::encode_lower(black_box(&input)))
    });
    group.finish();
}

fn encode_vs_base16_encode_lower_100kb(c: &mut Criterion) {
    let input: Vec<u8> = (0..100_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("checksummed_hex_encode_100kb");
    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("base16_encode_lower", |b| {
        b.iter(|| base16::encode_lower(black_box(&input)))
    });
    group.finish();
}

fn encode_vs_base16_encode_lower_1mb(c: &mut Criterion) {
    let input: Vec<u8> = (0..1_000_000).map(|_| rand::random::<u8>()).collect();
    let mut group = c.benchmark_group("checksummed_hex_encode_1mb");
    group.bench_function("encode", |b| b.iter(|| encode(black_box(&input))));
    group.bench_function("base16_encode_lower", |b| {
        b.iter(|| base16::encode_lower(black_box(&input)))
    });
    group.finish();
}

fn decode_vs_base16_decode_checksummed_hex_small(c: &mut Criterion) {
    let input: Vec<u8> = (0..SMALL_BYTES_COUNT)
        .map(|_| rand::random::<u8>())
        .collect();
    let input = encode(&input);
    let mut group = c.benchmark_group("checksummed_hex_decode_small");
    group.bench_function("decode", |b| b.iter(|| decode(black_box(&input))));
    group.bench_function("base16_decode", |b| {
        b.iter(|| base16::decode(black_box(&input)))
    });
    group.finish();
}

fn decode_vs_base16_decode_checksummed_hex_100kb(c: &mut Criterion) {
    let input: Vec<u8> = (0..100_000).map(|_| rand::random::<u8>()).collect();
    let input = encode(&input);
    let mut group = c.benchmark_group("checksummed_hex_decode_100kb");
    group.bench_function("decode", |b| b.iter(|| decode(black_box(&input))));
    group.bench_function("base16_decode", |b| {
        b.iter(|| base16::decode(black_box(&input)))
    });
    group.finish();
}

fn decode_vs_base16_decode_checksummed_hex_1mb(c: &mut Criterion) {
    let input: Vec<u8> = (0..1_000_000).map(|_| rand::random::<u8>()).collect();
    let input = encode(&input);
    let mut group = c.benchmark_group("checksummed_hex_decode_1mb");
    group.bench_function("decode", |b| b.iter(|| decode(black_box(&input))));
    group.bench_function("base16_decode", |b| {
        b.iter(|| base16::decode(black_box(&input)))
    });
    group.finish();
}

fn decode_vs_base16_decode_unchecked_hex(c: &mut Criterion) {
    let input: Vec<u8> = (0..SMALL_BYTES_COUNT)
        .map(|_| rand::random::<u8>())
        .collect();
    let input = base16::encode_lower(&input);
    let mut group = c.benchmark_group("checksummed_hex_decode_lowercase_hex");

    group.bench_function("decode", |b| b.iter(|| decode(black_box(&input))));
    group.bench_function("base16_decode", |b| {
        b.iter(|| base16::decode(black_box(&input)))
    });
    group.finish();
}

criterion_group!(
    benches,
    encode_vs_base16_encode_lower_small,
    encode_vs_base16_encode_lower_100kb,
    encode_vs_base16_encode_lower_1mb,
    decode_vs_base16_decode_checksummed_hex_small,
    decode_vs_base16_decode_checksummed_hex_100kb,
    decode_vs_base16_decode_checksummed_hex_1mb,
    decode_vs_base16_decode_unchecked_hex
);
criterion_main!(benches);
