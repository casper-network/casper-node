mod concurrent;
mod proptests;
mod simple;

use casper_types::bytesrepr::{Bytes, ToBytes};

use crate::{
    shared::newtypes::Blake2bHash,
    storage::trie::{Pointer, PointerBlock, Trie},
};

#[derive(Clone)]
struct TestData<K, V>(Blake2bHash, Trie<K, V>);

impl<'a, K, V> From<&'a TestData<K, V>> for (&'a Blake2bHash, &'a Trie<K, V>) {
    fn from(test_data: &'a TestData<K, V>) -> Self {
        (&test_data.0, &test_data.1)
    }
}

fn create_data() -> Vec<TestData<Bytes, Bytes>> {
    let leaf_1 = Trie::Leaf {
        key: Bytes::from(vec![0u8, 0, 0]),
        value: Bytes::from(b"val_1".to_vec()),
    };
    let leaf_2 = Trie::Leaf {
        key: Bytes::from(vec![1u8, 0, 0]),
        value: Bytes::from(b"val_2".to_vec()),
    };
    let leaf_3 = Trie::Leaf {
        key: Bytes::from(vec![1u8, 0, 1]),
        value: Bytes::from(b"val_3".to_vec()),
    };

    let leaf_1_hash = Blake2bHash::new(&leaf_1.to_bytes().unwrap());
    let leaf_2_hash = Blake2bHash::new(&leaf_2.to_bytes().unwrap());
    let leaf_3_hash = Blake2bHash::new(&leaf_3.to_bytes().unwrap());

    let node_2: Trie<Bytes, Bytes> = {
        let mut pointer_block = PointerBlock::new();
        pointer_block[0] = Some(Pointer::LeafPointer(leaf_2_hash));
        pointer_block[1] = Some(Pointer::LeafPointer(leaf_3_hash));
        let pointer_block = Box::new(pointer_block);
        Trie::Node { pointer_block }
    };

    let node_2_hash = Blake2bHash::new(&node_2.to_bytes().unwrap());

    let ext_node: Trie<Bytes, Bytes> = {
        let affix = vec![1u8, 0];
        let pointer = Pointer::NodePointer(node_2_hash);
        Trie::Extension {
            affix: affix.into(),
            pointer,
        }
    };

    let ext_node_hash = Blake2bHash::new(&ext_node.to_bytes().unwrap());

    let node_1: Trie<Bytes, Bytes> = {
        let mut pointer_block = PointerBlock::new();
        pointer_block[0] = Some(Pointer::LeafPointer(leaf_1_hash));
        pointer_block[1] = Some(Pointer::NodePointer(ext_node_hash));
        let pointer_block = Box::new(pointer_block);
        Trie::Node { pointer_block }
    };

    let node_1_hash = Blake2bHash::new(&node_1.to_bytes().unwrap());

    vec![
        TestData(leaf_1_hash, leaf_1),
        TestData(leaf_2_hash, leaf_2),
        TestData(leaf_3_hash, leaf_3),
        TestData(node_1_hash, node_1),
        TestData(node_2_hash, node_2),
        TestData(ext_node_hash, ext_node),
    ]
}
