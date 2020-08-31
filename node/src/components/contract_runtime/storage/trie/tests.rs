#[test]
fn radix_is_256() {
    assert_eq!(
        super::RADIX,
        256,
        "Changing RADIX alone might cause things to break"
    );
}

mod pointer_block {
    use crate::{components::contract_runtime::storage::trie::*, crypto::hash};

    /// A defense against changes to [`RADIX`](history::trie::RADIX).
    #[test]
    fn debug_formatter_succeeds() {
        let _ = format!("{:?}", PointerBlock::new());
    }

    #[test]
    fn assignment_and_indexing() {
        let test_hash = hash::hash(b"TrieTrieAgain");
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
        let test_hash = hash::hash(b"TrieTrieAgain");
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

    use casper_types::bytesrepr;

    use crate::components::contract_runtime::storage::trie::gens::*;

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
    }
}
