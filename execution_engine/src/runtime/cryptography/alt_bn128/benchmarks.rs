//! Manual benchmarks. Run optimized with --ignored --nocapture --test-threads=1.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use casper_types::{HostFunctionCostsV1, U256};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::*;

/// Return p50, p90, and p99 nanoseconds per call after warming up.
fn measure(mut operation: impl FnMut()) -> (f64, f64, f64) {
    let start = Instant::now();
    let mut warmup_iterations = 0;
    while start.elapsed() < Duration::from_millis(100) {
        operation();
        warmup_iterations += 1;
    }
    let ns_per_call = start.elapsed().as_nanos() / warmup_iterations.max(1);
    let iterations = (Duration::from_millis(5).as_nanos() / ns_per_call.max(1)).max(1);
    let mut samples = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = Instant::now();
        for _ in 0..iterations {
            operation();
        }
        samples.push(start.elapsed().as_nanos() as f64 / iterations as f64);
    }
    samples.sort_unstable_by(f64::total_cmp);
    (samples[49], samples[89], samples[98])
}

fn report(name: &str, operation: impl FnMut()) {
    let (p50, p90, p99) = measure(operation);
    println!("{name},{p50:.0},{p90:.0},{p99:.0}");
}

fn print_weights() {
    let costs = HostFunctionCostsV1::default();
    println!(
        "input weights (gas/byte): add={}, mul={}, pairing={}",
        costs.alt_bn128_add.arguments()[1],
        costs.alt_bn128_mul.arguments()[1],
        costs.alt_bn128_pairing.arguments()[1],
    );
}

fn nonzero_pairs(count: usize) -> Vec<u8> {
    let mut rng = ChaCha8Rng::from_seed([42; 32]);
    let mut input = Vec::with_capacity(count * PAIR_SIZE);
    for _ in 0..count {
        let a = G1::one() * Fr::random(&mut rng);
        let b = G2::one() * Fr::random(&mut rng);
        assert!(!a.is_zero());
        assert!(!b.is_zero());
        input.extend_from_slice(&write_point(a).unwrap());
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

#[test]
#[ignore = "manual gas calibration; run with --release --ignored --nocapture --test-threads=1"]
fn nonzero_pairing_batches() {
    assert!(
        !cfg!(debug_assertions),
        "gas calibration requires an optimized --release build"
    );
    let pairs = nonzero_pairs(64);
    let mut multiplication = pairs[..POINT_SIZE].to_vec();
    // A full-width scalar exercises the complete multiplication, unlike 0 or 1.
    multiplication.extend_from_slice(&[0x55; COORDINATE_SIZE]);
    assert!(mul(&multiplication).is_ok());
    println!("operation,p50_ns,p90_ns,p99_ns");
    report("mul/reference", || {
        black_box(mul(black_box(&multiplication)).unwrap());
    });
    for count in [0, 1, 2, 4, 8, 16, 32, 64] {
        let input = &pairs[..count * PAIR_SIZE];
        assert_eq!(pairing(input), Ok(count == 0));
        report(&format!("pairing/{count}"), || {
            black_box(pairing(black_box(input)).unwrap());
        });
    }
    print_weights();
}

#[test]
#[ignore = "manual benchmark; run with --release --ignored --nocapture --test-threads=1"]
fn addition_and_multiplication() {
    assert!(
        !cfg!(debug_assertions),
        "benchmarks require an optimized --release build"
    );
    let pairs = nonzero_pairs(64);
    let points: Vec<[u8; POINT_SIZE]> = pairs
        .chunks_exact(PAIR_SIZE)
        .map(|pair| pair[..POINT_SIZE].try_into().unwrap())
        .collect();
    let p = points[0];
    let q = points[1];
    let point_p = read_point(&p).unwrap();
    let negative_p = write_point(-point_p).unwrap();
    let infinity = [0; POINT_SIZE];

    println!("operation,p50_ns,p90_ns,p99_ns");
    for (name, left, right, expected) in [
        (
            "add/distinct_nonzero",
            p,
            q,
            write_point(point_p + read_point(&q).unwrap()).unwrap(),
        ),
        (
            "add/doubling",
            p,
            p,
            write_point(point_p + point_p).unwrap(),
        ),
        ("add/cancellation", p, negative_p, infinity),
        ("add/left_infinity", infinity, p, p),
        ("add/right_infinity", p, infinity, p),
        ("add/both_infinity", infinity, infinity, infinity),
    ] {
        let input = [left, right].concat();
        assert_eq!(add(&input), Ok(expected));
        report(name, || {
            black_box(add(black_box(&input)).unwrap());
        });
    }

    let addition_inputs: Vec<_> = points.chunks_exact(2).map(|pair| pair.concat()).collect();
    for input in &addition_inputs {
        assert_ne!(add(input).unwrap(), infinity);
    }
    let mut index = 0;
    report("add/random_corpus_32", || {
        black_box(add(black_box(&addition_inputs[index])).unwrap());
        index = (index + 1) % addition_inputs.len();
    });

    let order = U256::from_str_radix(
        "30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001",
        16,
    )
    .unwrap();
    for (name, value) in [
        ("mul/zero", U256::zero()),
        ("mul/one", U256::one()),
        ("mul/two", U256::from(2)),
        ("mul/u64_max", U256::from(u64::MAX)),
        ("mul/full_width_55", U256::from_little_endian(&[0x55; 32])),
        ("mul/full_width_aa", U256::from_little_endian(&[0xaa; 32])),
        ("mul/order_minus_one", order - U256::one()),
        ("mul/order", order),
        ("mul/order_plus_one", order + U256::one()),
        ("mul/max_u256", U256::MAX),
    ] {
        let mut scalar = [0; COORDINATE_SIZE];
        value.to_little_endian(&mut scalar);
        let input = [p.as_slice(), &scalar].concat();
        let expected =
            write_point(point_p * Fr::from_slice(&big_endian(&scalar).unwrap()).unwrap()).unwrap();
        assert_eq!(mul(&input), Ok(expected));
        report(name, || {
            black_box(mul(black_box(&input)).unwrap());
        });
    }
    let input = [infinity.as_slice(), &[0x55; COORDINATE_SIZE]].concat();
    assert_eq!(mul(&input), Ok(infinity));
    report("mul/infinity", || {
        black_box(mul(black_box(&input)).unwrap());
    });

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
    report("mul/random_full_width_corpus_32", || {
        black_box(mul(black_box(&multiplication_inputs[index])).unwrap());
        index = (index + 1) % multiplication_inputs.len();
    });
    print_weights();
}
