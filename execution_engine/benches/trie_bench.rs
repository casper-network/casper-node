use criterion::{black_box, criterion_group, criterion_main, Bencher, Criterion};

use casper_execution_engine::storage::trie::{Pointer, PointerBlock, Trie};
use casper_hashing::Digest;
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    CLValue, Key, StoredValue,
};

fn serialize_trie_leaf(b: &mut Bencher) {
    let leaf = Trie::Leaf {
        key: Key::Account(AccountHash::new([0; 32])),
        value: StoredValue::CLValue(CLValue::from_t(42_i32).unwrap()),
    };
    b.iter(|| ToBytes::to_bytes(black_box(&leaf)));
}

fn deserialize_trie_leaf(b: &mut Bencher) {
    let leaf = Trie::Leaf {
        key: Key::Account(AccountHash::new([0; 32])),
        value: StoredValue::CLValue(CLValue::from_t(42_i32).unwrap()),
    };
    let leaf_bytes = leaf.to_bytes().unwrap();
    b.iter(|| Trie::<Key, StoredValue>::from_bytes(black_box(&leaf_bytes)));
}

fn serialize_trie_node(b: &mut Bencher) {
    let node = Trie::<Key, StoredValue>::Node {
        pointer_block: Box::new(PointerBlock::default()),
    };
    b.iter(|| ToBytes::to_bytes(black_box(&node)));
}

fn deserialize_trie_node(b: &mut Bencher) {
    let node = Trie::<Key, StoredValue>::Node {
        pointer_block: Box::new(PointerBlock::default()),
    };
    let node_bytes = node.to_bytes().unwrap();

    b.iter(|| Trie::<Key, StoredValue>::from_bytes(black_box(&node_bytes)));
}

fn serialize_trie_node_pointer(b: &mut Bencher) {
    let node = Trie::<Key, StoredValue>::Extension {
        affix: (0..255).collect(),
        pointer: Pointer::NodePointer(Digest::hash(&[0; 32])),
    };

    b.iter(|| ToBytes::to_bytes(black_box(&node)));
}

fn deserialize_trie_node_pointer(b: &mut Bencher) {
    let node = Trie::<Key, StoredValue>::Extension {
        affix: (0..255).collect(),
        pointer: Pointer::NodePointer(Digest::hash(&[0; 32])),
    };
    let node_bytes = node.to_bytes().unwrap();

    b.iter(|| Trie::<Key, StoredValue>::from_bytes(black_box(&node_bytes)));
}

fn trie_bench(c: &mut Criterion) {
    c.bench_function("serialize_trie_leaf", serialize_trie_leaf);
    c.bench_function("deserialize_trie_leaf", deserialize_trie_leaf);
    c.bench_function("serialize_trie_node", serialize_trie_node);
    c.bench_function("deserialize_trie_node", deserialize_trie_node);
    c.bench_function("serialize_trie_node_pointer", serialize_trie_node_pointer);
    c.bench_function(
        "deserialize_trie_node_pointer",
        deserialize_trie_node_pointer,
    );
}

criterion_group!(benches, trie_bench);
criterion_main!(benches);
