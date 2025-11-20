use std::str::FromStr;

use bytes::Bytes;
use casper_types::{
    bytesrepr::{self, Bytes as BytesreprBytes, ToBytes},
    crypto::{SecretKey, Signature},
    HashAlgorithm, U256,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

#[path = "../src/host/crypto.rs"]
mod crypto;

fn u256_to_le_bytes(value: U256) -> [u8; 32] {
    let mut buf = [0u8; 32];
    value.to_little_endian(&mut buf);
    buf
}

fn generic_hash_input() -> Bytes {
    let message = b"benchmark-generic-hash-payload";
    let payload = BytesreprBytes::from(message.to_vec());
    let data = (HashAlgorithm::Blake2b as u32, payload);
    let serialized = bytesrepr::serialize(&data).expect("serialize generic_hash input");
    Bytes::from(serialized)
}

fn recover_secp256k1_input() -> (Bytes, u32) {
    let message = b"benchmark-secp256k1-message";
    let secret_key = SecretKey::generate_secp256k1().expect("generate secp256k1 key");
    let (signature, recovery_id) = match secret_key {
        SecretKey::Secp256k1(signing_key) => signing_key.sign_recoverable(message).unwrap(),
        _ => unreachable!(),
    };
    let signature = Signature::Secp256k1(signature);
    let recovery_id = recovery_id.to_byte() as u32;
    let signature_bytes = signature.to_bytes().expect("signature bytes");

    let input = (
        recovery_id,
        BytesreprBytes::from(message.to_vec()),
        BytesreprBytes::from(signature_bytes),
    );
    let serialized = bytesrepr::serialize(&input).expect("serialize recover_secp256k1 input");
    (Bytes::from(serialized), recovery_id)
}

fn alt_bn128_add_input() -> Bytes {
    let x1 = U256::from_str(
        "18b18acfb4c2c30276db5411368e7185b311dd124691610c5d3b74034e093dc9",
    )
    .expect("u256 x1");
    let y1 = U256::from_str(
        "063c909c4720840cb5134cb9f59fa749755796819658d32efc0d288198f37266",
    )
    .expect("u256 y1");
    let x2 = U256::from_str(
        "07c2b7f58a84bd6145f00c9c2bc0bb1a187f20ff2c92963a88019e7c6a014eed",
    )
    .expect("u256 x2");
    let y2 = U256::from_str(
        "06614e20c147e940f2d70da3f74c9a17df361706a4485c742bd6788478fa17d7",
    )
    .expect("u256 y2");

    let data = (
        u256_to_le_bytes(x1),
        u256_to_le_bytes(y1),
        u256_to_le_bytes(x2),
        u256_to_le_bytes(y2),
    );
    let serialized = bytesrepr::serialize(&data).expect("serialize alt_bn128_add input");
    Bytes::from(serialized)
}

fn alt_bn128_mul_input() -> Bytes {
    let x = U256::from_str(
        "2bd3e6d0f3b142924f5ca7b49ce5b9d54c4703d7ae5648e61d02268b1a0a9fb7",
    )
    .expect("u256 x");
    let y = U256::from_str(
        "21611ce0a6af85915e2f1d70300909ce2e49dfad4a4619c8390cae66cefdb204",
    )
    .expect("u256 y");
    let scalar = U256::from_str(
        "00000000000000000000000000000000000000000000000011138ce750fa15c2",
    )
    .expect("u256 scalar");

    let data = (
        u256_to_le_bytes(x),
        u256_to_le_bytes(y),
        u256_to_le_bytes(scalar),
    );
    let serialized = bytesrepr::serialize(&data).expect("serialize alt_bn128_mul input");
    Bytes::from(serialized)
}

fn alt_bn128_pairing_input() -> Bytes {
    let zero = [0u8; 32];
    let pair = crypto::Pair {
        ax: zero,
        ay: zero,
        bax: zero,
        bay: zero,
        bbx: zero,
        bby: zero,
    };
    // Manual bytesrepr-style serialization of Vec<Pair>.
    let mut buf = Vec::with_capacity(4 + 6 * 32);
    buf.extend_from_slice(&(1u32).to_le_bytes());
    buf.extend_from_slice(&pair.ax);
    buf.extend_from_slice(&pair.ay);
    buf.extend_from_slice(&pair.bax);
    buf.extend_from_slice(&pair.bay);
    buf.extend_from_slice(&pair.bbx);
    buf.extend_from_slice(&pair.bby);
    Bytes::from(buf)
}

fn bench_generic_hash(c: &mut Criterion) {
    let input = generic_hash_input();
    c.bench_function("host_generic_hash", |b| {
        b.iter(|| {
            let res = crypto::host_generic_hash(black_box(input.clone())).expect("host call");
            black_box(res)
        })
    });
}

fn bench_recover_secp256k1(c: &mut Criterion) {
    let (input, _recovery_id) = recover_secp256k1_input();
    c.bench_function("host_recover_secp256k1", |b| {
        b.iter(|| {
            let res = crypto::host_recover_secp256k1(black_box(input.clone())).expect("host call");
            black_box(res)
        })
    });
}

fn bench_alt_bn128_add(c: &mut Criterion) {
    let input = alt_bn128_add_input();
    c.bench_function("host_alt_bn128_add", |b| {
        b.iter(|| {
            let res = crypto::host_alt_bn128_add(black_box(input.clone())).expect("host call");
            black_box(res)
        })
    });
}

fn bench_alt_bn128_mul(c: &mut Criterion) {
    let input = alt_bn128_mul_input();
    c.bench_function("host_alt_bn128_mul", |b| {
        b.iter(|| {
            let res = crypto::host_alt_bn128_mul(black_box(input.clone())).expect("host call");
            black_box(res)
        })
    });
}

fn bench_alt_bn128_pairing(c: &mut Criterion) {
    let input = alt_bn128_pairing_input();
    c.bench_function("host_alt_bn128_pairing", |b| {
        b.iter(|| {
            let res = crypto::host_alt_bn128_pairing(black_box(input.clone())).expect("host call");
            black_box(res)
        })
    });
}

criterion_group!(
    benches,
    bench_generic_hash,
    bench_recover_secp256k1,
    bench_alt_bn128_add,
    bench_alt_bn128_mul,
    bench_alt_bn128_pairing
);
criterion_main!(benches);
