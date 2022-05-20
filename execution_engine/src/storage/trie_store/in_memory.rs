//! An in-memory trie store, intended to be used for testing.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use casper_types::bytesrepr::Bytes;

use super::{Digest, Store, Trie, TrieStore};
use crate::storage::{
    error::in_memory::Error,
    store::{BytesReader, BytesWriter, ErrorSource},
};

/// An in-memory trie store.
pub struct InMemoryTrieStore {
    /// Data backing the trie store.
    pub data: Arc<Mutex<HashMap<Bytes, Bytes>>>,
}

impl ErrorSource for InMemoryTrieStore {
    type Error = Error;
}

impl BytesReader for InMemoryTrieStore {
    fn read_bytes(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        let bytes = self.data.lock().unwrap().get(&Bytes::from(key)).cloned();
        Ok(bytes)
    }
}

impl BytesWriter for InMemoryTrieStore {
    fn write_bytes(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        self.data
            .lock()
            .unwrap()
            .insert(Bytes::from(key), Bytes::from(value));
        Ok(())
    }
}

impl InMemoryTrieStore {
    pub(crate) fn new() -> Self {
        InMemoryTrieStore {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<K, V> Store<Digest, Trie<K, V>> for InMemoryTrieStore {}
impl<K, V> TrieStore<K, V> for InMemoryTrieStore {}

#[cfg(test)]
mod tests {
    use casper_hashing::Digest;
    use casper_types::bytesrepr::{Bytes, ToBytes};

    use crate::storage::{
        store::Store,
        trie::{Pointer, PointerBlock, Trie},
        trie_store::in_memory::InMemoryTrieStore,
    };

    #[test]
    fn test_in_memory_trie_store() {
        // Create some leaves
        let leaf_1 = Trie::Leaf {
            key: Bytes::from(vec![0u8, 0, 0]),
            value: Bytes::from(b"val_1".to_vec()),
        };
        let leaf_2 = Trie::Leaf {
            key: Bytes::from(vec![1u8, 0, 0]),
            value: Bytes::from(b"val_2".to_vec()),
        };

        // Get their hashes
        let leaf_1_hash = Digest::hash(&leaf_1.to_bytes().unwrap());
        let leaf_2_hash = Digest::hash(&leaf_2.to_bytes().unwrap());

        // Create a node
        let node: Trie<Bytes, Bytes> = {
            let mut pointer_block = PointerBlock::new();
            pointer_block[0] = Some(Pointer::LeafPointer(leaf_1_hash));
            pointer_block[1] = Some(Pointer::LeafPointer(leaf_2_hash));
            let pointer_block = Box::new(pointer_block);
            Trie::Node { pointer_block }
        };

        // Get its hash
        let node_hash = Digest::hash(&node.to_bytes().unwrap());

        let store = InMemoryTrieStore::new();

        // Observe that nothing has been persisted to the store
        for hash in vec![&leaf_1_hash, &leaf_2_hash, &node_hash].iter() {
            // We need to use a type annotation here to help the compiler choose
            // a suitable FromBytes instance
            let maybe_trie: Option<Trie<Bytes, Bytes>> = store.get(hash).unwrap();
            assert!(maybe_trie.is_none());
        }

        // Put the values in the store
        store.put(&leaf_1_hash, &leaf_1).unwrap();
        store.put(&leaf_2_hash, &leaf_2).unwrap();
        store.put(&node_hash, &node).unwrap();

        // Now let's check to see if the values were stored again
        // Get the values in the store
        assert_eq!(Some(leaf_1), store.get(&leaf_1_hash).unwrap());
        assert_eq!(Some(leaf_2), store.get(&leaf_2_hash).unwrap());
        assert_eq!(Some(node), store.get(&node_hash).unwrap());
    }
}
