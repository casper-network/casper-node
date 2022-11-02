mod concurrent;
mod proptests;
mod simple;

use casper_hashing::Digest;
use casper_types::bytesrepr::ToBytes;

use crate::global_state::storage::{
    trie::{Pointer, PointerBlock, Trie},
    trie_store::operations::{test_key, test_value},
};

#[derive(Clone)]
struct TestData(Digest, Trie);

impl<'a> From<&'a TestData> for (&'a Digest, &'a Trie) {
    fn from(test_data: &'a TestData) -> Self {
        (&test_data.0, &test_data.1)
    }
}

fn create_data() -> Vec<TestData> {
    let leaf_1 = Trie::Leaf {
        key: test_key([0u8, 0, 0, 0, 0, 0, 0]),
        value: test_value(b"value1".to_owned()),
    };
    let leaf_2 = Trie::Leaf {
        key: test_key([1u8, 0, 0, 0, 0, 0, 0]),
        value: test_value(b"value2".to_owned()),
    };
    let leaf_3 = Trie::Leaf {
        key: test_key([1u8, 0, 1, 0, 0, 0, 0]),
        value: test_value(b"value3".to_owned()),
    };

    let leaf_1_hash = Digest::hash(&leaf_1.to_bytes().unwrap());
    let leaf_2_hash = Digest::hash(&leaf_2.to_bytes().unwrap());
    let leaf_3_hash = Digest::hash(&leaf_3.to_bytes().unwrap());

    let node_2: Trie = {
        let mut pointer_block = PointerBlock::new();
        pointer_block[0] = Some(Pointer::LeafPointer(leaf_2_hash));
        pointer_block[1] = Some(Pointer::LeafPointer(leaf_3_hash));
        let pointer_block = Box::new(pointer_block);
        Trie::Node { pointer_block }
    };

    let node_2_hash = Digest::hash(&node_2.to_bytes().unwrap());

    let ext_node: Trie = {
        let affix = vec![1u8, 0];
        let pointer = Pointer::NodePointer(node_2_hash);
        Trie::Extension {
            affix: affix.into(),
            pointer,
        }
    };

    let ext_node_hash = Digest::hash(&ext_node.to_bytes().unwrap());

    let node_1: Trie = {
        let mut pointer_block = PointerBlock::new();
        pointer_block[0] = Some(Pointer::LeafPointer(leaf_1_hash));
        pointer_block[1] = Some(Pointer::NodePointer(ext_node_hash));
        let pointer_block = Box::new(pointer_block);
        Trie::Node { pointer_block }
    };

    let node_1_hash = Digest::hash(&node_1.to_bytes().unwrap());

    vec![
        TestData(leaf_1_hash, leaf_1),
        TestData(leaf_2_hash, leaf_2),
        TestData(leaf_3_hash, leaf_3),
        TestData(node_1_hash, node_1),
        TestData(node_2_hash, node_2),
        TestData(ext_node_hash, ext_node),
    ]
}
