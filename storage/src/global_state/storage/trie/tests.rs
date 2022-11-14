#[test]
fn radix_is_256() {
    assert_eq!(
        super::RADIX,
        256,
        "Changing RADIX alone might cause things to break"
    );
}

mod pointer_block {
    use crate::global_state::storage::trie::*;

    /// A defense against changes to [`RADIX`](history::trie::RADIX).
    #[test]
    fn debug_formatter_succeeds() {
        let _ = format!("{:?}", PointerBlock::new());
    }

    #[test]
    fn assignment_and_indexing() {
        let test_hash = Digest::hash(b"TrieTrieAgain");
        let leaf_pointer = Some(Pointer::LeafPointer(test_hash));
        let mut pointer_block = PointerBlock::new();
        pointer_block[0] = leaf_pointer;
        pointer_block[RADIX - 1] = leaf_pointer;
        assert_eq!(leaf_pointer, pointer_block[0]);
        assert_eq!(leaf_pointer, pointer_block[RADIX - 1]);
        assert_eq!(None, pointer_block[1]);
        assert_eq!(None, pointer_block[RADIX - 2]);
    }

    #[test]
    #[should_panic]
    fn assignment_off_end() {
        let test_hash = Digest::hash(b"TrieTrieAgain");
        let leaf_pointer = Some(Pointer::LeafPointer(test_hash));
        let mut pointer_block = PointerBlock::new();
        pointer_block[RADIX] = leaf_pointer;
    }

    #[test]
    #[should_panic]
    fn indexing_off_end() {
        let pointer_block = PointerBlock::new();
        let _val = pointer_block[RADIX];
    }
}

mod proptests {
    use proptest::prelude::*;

    use casper_hashing::Digest;
    use casper_types::{bytesrepr, gens::key_arb, Key, StoredValue};

    use crate::global_state::storage::trie::{gens::*, PointerBlock, Trie};

    proptest! {
        #[test]
        fn roundtrip_blake2b_hash(hash in blake2b_hash_arb()) {
            bytesrepr::test_serialization_roundtrip(&hash);
        }

        #[test]
        fn roundtrip_trie_pointer(pointer in trie_pointer_arb()) {
            bytesrepr::test_serialization_roundtrip(&pointer);
        }

        #[test]
        fn roundtrip_trie_pointer_block(pointer_block in trie_pointer_block_arb()) {
            bytesrepr::test_serialization_roundtrip(&pointer_block);
        }

        #[test]
        fn bytesrepr_roundtrip_trie_leaf(trie_leaf in trie_leaf_arb()) {
            bytesrepr::test_serialization_roundtrip(&trie_leaf);
        }

        #[test]
        fn bytesrepr_roundtrip_trie_extension(trie_extension in trie_extension_arb()) {
            bytesrepr::test_serialization_roundtrip(&trie_extension);
        }

        #[test]
        fn bytesrepr_roundtrip_trie_node(trie_node in trie_node_arb()) {
            bytesrepr::test_serialization_roundtrip(&trie_node);
        }

        #[test]
        fn roundtrip_key(key in key_arb()) {
            bytesrepr::test_serialization_roundtrip(&key);
        }

        #[test]
        fn serde_roundtrip_trie_pointer_block(pointer_block in trie_pointer_block_arb()) {
             let json_str = serde_json::to_string(&pointer_block)?;
             let deserialized_pointer_block: PointerBlock = serde_json::from_str(&json_str)?;
             assert_eq!(pointer_block, deserialized_pointer_block)
        }

        #[test]
        fn serde_roundtrip_trie_leaf(trie_leaf in trie_leaf_arb()) {
             let json_str = serde_json::to_string(&trie_leaf)?;
             let deserialized_trie: Trie<Key, StoredValue> = serde_json::from_str(&json_str)?;
             assert_eq!(trie_leaf, deserialized_trie)
        }

        #[test]
        fn serde_roundtrip_trie_node(trie_node in trie_node_arb()) {
             let json_str = serde_json::to_string(&trie_node)?;
             let deserialized_trie: Trie<Key, StoredValue> = serde_json::from_str(&json_str)?;
             assert_eq!(trie_node, deserialized_trie)
        }

        #[test]
        fn serde_roundtrip_trie_extension(trie_extension in trie_extension_arb()) {
             let json_str = serde_json::to_string(&trie_extension)?;
             let deserialized_trie: Trie<Key, StoredValue> = serde_json::from_str(&json_str)?;
             assert_eq!(trie_extension, deserialized_trie)
        }

        #[test]
        fn bincode_roundtrip_trie_leaf(trie_leaf in trie_leaf_arb()) {
           let bincode_bytes = bincode::serialize(&trie_leaf)?;
           let deserialized_trie = bincode::deserialize(&bincode_bytes)?;
           assert_eq!(trie_leaf, deserialized_trie)
        }

        #[test]
        fn bincode_roundtrip_trie_node(trie_node in trie_node_arb()) {
           let bincode_bytes = bincode::serialize(&trie_node)?;
           let deserialized_trie = bincode::deserialize(&bincode_bytes)?;
           assert_eq!(trie_node, deserialized_trie)
        }

        #[test]
        fn bincode_roundtrip_trie_extension(trie_extension in trie_extension_arb()) {
           let bincode_bytes = bincode::serialize(&trie_extension)?;
           let deserialized_trie = bincode::deserialize(&bincode_bytes)?;
           assert_eq!(trie_extension, deserialized_trie)
        }

        #[test]
        fn bincode_roundtrip_trie_pointer_block(pointer_block in trie_pointer_block_arb()) {
             let bincode_bytes = bincode::serialize(&pointer_block)?;
             let deserialized_pointer_block = bincode::deserialize(&bincode_bytes)?;
             assert_eq!(pointer_block, deserialized_pointer_block)
        }

        #[test]
        fn bincode_roundtrip_key(key in key_arb()) {
             let bincode_bytes = bincode::serialize(&key)?;
             let deserialized_key = bincode::deserialize(&bincode_bytes)?;
             prop_assert_eq!(key, deserialized_key)
        }

        #[test]
        fn serde_roundtrip_key(key in key_arb()) {
             let json_str = serde_json::to_string(&key)?;
             let deserialized_key = serde_json::from_str(&json_str)?;
             assert_eq!(key, deserialized_key)
        }

        #[test]
        fn iter_descendants_trie_leaf(trie_leaf in trie_leaf_arb()) {
            assert!(trie_leaf.iter_descendants().next().is_none());
        }

        #[test]
        fn iter_descendants_trie_extension(trie_extension in trie_extension_arb()) {
            let children = if let Trie::Extension { pointer, .. } = trie_extension {
                vec![*pointer.hash()]
            } else {
                unreachable!()
            };
            assert_eq!(children, trie_extension.iter_descendants().collect::<Vec<Digest>>());
        }

        #[test]
        fn iter_descendants_trie_node(trie_node in trie_node_arb()) {
            let children: Vec<Digest> = trie_node.as_pointer_block().unwrap()
                    .as_indexed_pointers()
                    .map(|(_index, ptr)| *ptr.hash())
                    .collect();
            assert_eq!(children, trie_node.iter_descendants().collect::<Vec<Digest>>());
        }
    }
}
