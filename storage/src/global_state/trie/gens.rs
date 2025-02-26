//! Generators for trie related types.
use proptest::{collection::vec, option, prelude::*};

use casper_types::{
    gens::{key_arb, stored_value_arb, trie_pointer_arb},
    Key, StoredValue,
};

use super::{Pointer, PointerBlock, Trie};

/// Generates a trie pointer block.
pub fn trie_pointer_block_arb() -> impl Strategy<Value = PointerBlock> {
    vec(option::of(trie_pointer_arb()), 256).prop_map(|vec| {
        let mut ret: [Option<Pointer>; 256] = [Default::default(); 256];
        ret.clone_from_slice(vec.as_slice());
        ret.into()
    })
}

/// Generates a trie leaf.
pub fn trie_leaf_arb() -> impl Strategy<Value = Trie<Key, StoredValue>> {
    (key_arb(), stored_value_arb()).prop_map(|(key, value)| Trie::Leaf { key, value })
}

/// Generates a trie node with a single child.
pub fn trie_extension_arb() -> impl Strategy<Value = Trie<Key, StoredValue>> {
    (vec(any::<u8>(), 0..32), trie_pointer_arb())
        .prop_map(|(affix, pointer)| Trie::extension(affix, pointer))
}

/// Generates a trie node with multiple children.
pub fn trie_node_arb() -> impl Strategy<Value = Trie<Key, StoredValue>> {
    trie_pointer_block_arb().prop_map(|pointer_block| Trie::Node {
        pointer_block: Box::new(pointer_block),
    })
}
