use std::hint::black_box;

use bn::{AffineG1, AffineG2, Fr, Group, G1, G2};
use casper_execution_engine::runtime::cryptography::alt_bn128::{add, mul, pairing};
use casper_types::U256;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

const COORDINATE_SIZE: usize = 32;
const POINT_SIZE: usize = 2 * COORDINATE_SIZE;
const PAIR_SIZE: usize = 6 * COORDINATE_SIZE;

fn encode_point(point: G1) -> [u8; POINT_SIZE] {
    let mut output = [0; POINT_SIZE];
    if let Some(point) = AffineG1::from_jacobian(point) {
        for (coordinate, dest) in [point.x(), point.y()]
            .into_iter()
            .zip(output.as_chunks_mut::<COORDINATE_SIZE>().0)
        {
            coordinate.to_big_endian(dest).unwrap();
            dest.reverse();
        }
    }
    output
}

fn nonzero_pairs(count: usize) -> Vec<(G1, G2)> {
    let mut rng = ChaCha8Rng::from_seed([42; 32]);
    (0..count)
        .map(|_| {
            let a = G1::one() * Fr::random(&mut rng);
            let b = G2::one() * Fr::random(&mut rng);
            assert!(!a.is_zero());
            assert!(!b.is_zero());
            (a, b)
        })
        .collect()
}

fn pairing_input(pairs: &[(G1, G2)]) -> Vec<u8> {
    let mut input = Vec::with_capacity(pairs.len() * PAIR_SIZE);
    for &(a, b) in pairs {
        input.extend_from_slice(&encode_point(a));
        let b = AffineG2::from_jacobian(b).unwrap();
        for coordinate in [
            b.x().real(),
            b.x().imaginary(),
            b.y().real(),
            b.y().imaginary(),
        ] {
            let mut bytes = [0; COORDINATE_SIZE];
            coordinate.to_big_endian(&mut bytes).unwrap();
            bytes.reverse();
            input.extend_from_slice(&bytes);
        }
    }
    input
}

fn addition_and_multiplication(c: &mut Criterion) {
    let pairs = nonzero_pairs(64);
    let points: Vec<_> = pairs
        .iter()
        .map(|&(point, _)| encode_point(point))
        .collect();
    let p = points[0];
    let q = points[1];
    let point_p = pairs[0].0;
    let negative_p = encode_point(-point_p);
    let infinity = [0; POINT_SIZE];

    let mut group = c.benchmark_group("alt_bn128/add");
    for (name, left, right, expected) in [
        ("distinct_nonzero", p, q, encode_point(point_p + pairs[1].0)),
        ("doubling", p, p, encode_point(point_p + point_p)),
        ("cancellation", p, negative_p, infinity),
        ("left_infinity", infinity, p, p),
        ("right_infinity", p, infinity, p),
        ("both_infinity", infinity, infinity, infinity),
    ] {
        let input = [left, right].concat();
        assert_eq!(add(&input), Ok(expected));
        group.bench_function(name, |b| b.iter(|| add(black_box(&input)).unwrap()));
    }

    let addition_inputs: Vec<_> = points.chunks_exact(2).map(|pair| pair.concat()).collect();
    for input in &addition_inputs {
        assert_ne!(add(input).unwrap(), infinity);
    }
    let mut index = 0;
    group.bench_function("random_corpus_32", |b| {
        b.iter(|| {
            let result = add(black_box(&addition_inputs[index])).unwrap();
            index = (index + 1) % addition_inputs.len();
            result
        });
    });
    group.finish();

    let mut group = c.benchmark_group("alt_bn128/mul");
    let order = U256::from_str_radix(
        "30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001",
        16,
    )
    .unwrap();
    for (name, value) in [
        ("zero", U256::zero()),
        ("one", U256::one()),
        ("two", U256::from(2)),
        ("u64_max", U256::from(u64::MAX)),
        ("full_width_55", U256::from_little_endian(&[0x55; 32])),
        ("full_width_aa", U256::from_little_endian(&[0xaa; 32])),
        ("order_minus_one", order - U256::one()),
        ("order", order),
        ("order_plus_one", order + U256::one()),
        ("max_u256", U256::MAX),
    ] {
        let mut scalar = [0; COORDINATE_SIZE];
        value.to_little_endian(&mut scalar);
        let input = [p.as_slice(), &scalar].concat();
        scalar.reverse();
        let expected = encode_point(point_p * Fr::from_slice(&scalar).unwrap());
        assert_eq!(mul(&input), Ok(expected));
        group.bench_function(name, |b| b.iter(|| mul(black_box(&input)).unwrap()));
    }
    let input = [infinity.as_slice(), &[0x55; COORDINATE_SIZE]].concat();
    assert_eq!(mul(&input), Ok(infinity));
    group.bench_function("infinity", |b| b.iter(|| mul(black_box(&input)).unwrap()));

    let mut rng = ChaCha8Rng::from_seed([7; 32]);
    let multiplication_inputs: Vec<_> = points[..32]
        .iter()
        .map(|point| {
            let mut scalar = [0; COORDINATE_SIZE];
            rng.fill_bytes(&mut scalar);
            scalar[31] |= 0x80;
            let input = [point.as_slice(), &scalar].concat();
            assert_ne!(mul(&input).unwrap(), infinity);
            input
        })
        .collect();
    let mut index = 0;
    group.bench_function("random_full_width_corpus_32", |b| {
        b.iter(|| {
            let result = mul(black_box(&multiplication_inputs[index])).unwrap();
            index = (index + 1) % multiplication_inputs.len();
            result
        });
    });
    group.finish();
}

fn nonzero_pairing_batches(c: &mut Criterion) {
    let pairs = nonzero_pairs(64);
    let mut group = c.benchmark_group("alt_bn128/pairing");
    group.sampling_mode(SamplingMode::Flat);
    for count in [0, 1, 2, 4, 8, 16, 32, 64] {
        let input = pairing_input(&pairs[..count]);
        assert_eq!(pairing(&input), Ok(count == 0));
        group.bench_with_input(BenchmarkId::from_parameter(count), &input, |b, input| {
            b.iter(|| pairing(black_box(input)).unwrap());
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    addition_and_multiplication,
    nonzero_pairing_batches
);
criterion_main!(benches);
