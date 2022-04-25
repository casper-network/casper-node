use criterion::{black_box, criterion_group, criterion_main, Bencher, Criterion};

use casper_types::U512;

use casper_node::capnp::{FromCapnpBytes, ToCapnpBytes};

fn capnp_serialize_u512(b: &mut Bencher) {
    let num_u512 = U512::default();
    b.iter(|| black_box(num_u512.try_to_capnp_bytes()));
}

fn capnp_deserialize_u512(b: &mut Bencher) {
    let num_u512 = U512::default();
    let num_u512_bytes = num_u512.try_to_capnp_bytes().unwrap();

    b.iter(|| U512::try_from_capnp_bytes(black_box(&num_u512_bytes)))
}

fn capnproto_bench(c: &mut Criterion) {
    c.bench_function("capnp_serialize_u512", capnp_serialize_u512);
    c.bench_function("capnp_deserialize_u512", capnp_deserialize_u512);
}

criterion_group!(benches, capnproto_bench);
criterion_main!(benches);
