#[test]
fn radix_is_256() {
    assert_eq!(
        super::RADIX,
        256,
        "Changing RADIX alone might cause things to break"
    );
}

mod pointer_block {
    use crate::storage::trie::*;

    /// A defense against changes to [`RADIX`](history::trie::RADIX).
    #[test]
    fn debug_formatter_succeeds() {
        let _ = format!("{:?}", PointerBlock::new());
    }

    #[test]
    fn assignment_and_indexing() {
        let test_hash = Blake2bHash::new(b"TrieTrieAgain");
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
        let test_hash = Blake2bHash::new(b"TrieTrieAgain");
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
    use proptest::prelude::proptest;

    use casper_types::{bytesrepr, Key};

    use crate::{
        shared::stored_value::StoredValue,
        storage::trie::{gens::*, PointerBlock, Trie},
    };

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
        fn roundtrip_trie(trie in trie_arb()) {
            bytesrepr::test_serialization_roundtrip(&trie);
        }

        #[test]
        fn serde_roundtrip_trie_pointer_block(pointer_block in trie_pointer_block_arb()) {
             let json_str = serde_json::to_string(&pointer_block)?;
             let deserialized_pointer_block: PointerBlock = serde_json::from_str(&json_str)?;
             assert_eq!(pointer_block, deserialized_pointer_block)
        }

        #[test]
        fn serde_roundtrip_trie(trie in trie_arb()) {
             let json_str = serde_json::to_string(&trie)?;
             let deserialized_trie: Trie<Key, StoredValue> = serde_json::from_str(&json_str)?;
             assert_eq!(trie, deserialized_trie)
        }
    }
}
