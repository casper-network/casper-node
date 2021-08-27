use proptest::{collection::vec, option, prelude::*};

use casper_types::{
    gens::{key_arb, stored_value_arb},
    Digest, Key, StoredValue,
};

use super::{Pointer, PointerBlock, Trie};

pub fn blake2b_hash_arb() -> impl Strategy<Value = Digest> {
    vec(any::<u8>(), 0..1000).prop_map(|b| Digest::hash(&b))
}

pub fn trie_pointer_arb() -> impl Strategy<Value = Pointer> {
    prop_oneof![
        blake2b_hash_arb().prop_map(Pointer::LeafPointer),
        blake2b_hash_arb().prop_map(Pointer::NodePointer)
    ]
}

pub fn trie_pointer_block_arb() -> impl Strategy<Value = PointerBlock> {
    vec(option::of(trie_pointer_arb()), 256).prop_map(|vec| {
        let mut ret: [Option<Pointer>; 256] = [Default::default(); 256];
        ret.clone_from_slice(vec.as_slice());
        ret.into()
    })
}

pub fn trie_arb() -> impl Strategy<Value = Trie<Key, StoredValue>> {
    prop_oneof![
        (key_arb(), stored_value_arb()).prop_map(|(key, value)| Trie::Leaf { key, value }),
        trie_pointer_block_arb().prop_map(|pointer_block| Trie::Node {
            pointer_block: Box::new(pointer_block)
        }),
        (vec(any::<u8>(), 0..32), trie_pointer_arb()).prop_map(|(affix, pointer)| {
            Trie::Extension {
                affix: affix.into(),
                pointer,
            }
        })
    ]
}
