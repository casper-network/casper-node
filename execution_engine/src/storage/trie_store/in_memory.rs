//! An in-memory trie store, intended to be used for testing.

use super::{Digest, Store, Trie, TrieStore, NAME};
use crate::storage::{error::in_memory::Error, transaction_source::in_memory::InMemoryEnvironment};

/// An in-memory trie store.
pub struct InMemoryTrieStore {
    maybe_name: Option<String>,
}

impl InMemoryTrieStore {
    pub(crate) fn new(_env: &InMemoryEnvironment, maybe_name: Option<&str>) -> Self {
        let name = maybe_name
            .map(|name| format!("{}-{}", NAME, name))
            .unwrap_or_else(|| String::from(NAME));
        InMemoryTrieStore {
            maybe_name: Some(name),
        }
    }
}

impl<K, V> Store<Digest, Trie<K, V>> for InMemoryTrieStore {
    type Error = Error;

    type Handle = Option<String>;

    fn handle(&self) -> Self::Handle {
        self.maybe_name.to_owned()
    }
}

impl<K, V> TrieStore<K, V> for InMemoryTrieStore {}

#[cfg(test)]
mod tests {
    use casper_hashing::Digest;
    use casper_types::bytesrepr::{Bytes, ToBytes};

    use crate::storage::{
        store::Store,
        transaction_source::{in_memory::InMemoryEnvironment, Transaction, TransactionSource},
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

        // Create the environment and the store. For both the in-memory and
        // LMDB-backed implementations, the environment is the source of
        // transactions.
        let env = InMemoryEnvironment::new();
        let store = InMemoryTrieStore::new(&env, None);

        // First let's create a read-write transaction, persist the values, but
        // forget to commit the transaction.
        {
            // Create a read-write transaction
            let mut txn = env.create_read_write_txn().unwrap();

            // Put the values in the store
            store.put(&mut txn, &leaf_1_hash, &leaf_1).unwrap();
            store.put(&mut txn, &leaf_2_hash, &leaf_2).unwrap();
            store.put(&mut txn, &node_hash, &node).unwrap();

            // Here we forget to commit the transaction before it goes out of scope
        }

        // Now let's check to see if the values were stored
        {
            // Create a read transaction
            let txn = env.create_read_txn().unwrap();

            // Observe that nothing has been persisted to the store
            for hash in vec![&leaf_1_hash, &leaf_2_hash, &node_hash].iter() {
                // We need to use a type annotation here to help the compiler choose
                // a suitable FromBytes instance
                let maybe_trie: Option<Trie<Bytes, Bytes>> = store.get(&txn, hash).unwrap();
                assert!(maybe_trie.is_none());
            }

            // Commit the read transaction.  Not strictly necessary, but better to be hygienic.
            txn.commit().unwrap();
        }

        // Now let's try that again, remembering to commit the transaction this time
        {
            // Create a read-write transaction
            let mut txn = env.create_read_write_txn().unwrap();

            // Put the values in the store
            store.put(&mut txn, &leaf_1_hash, &leaf_1).unwrap();
            store.put(&mut txn, &leaf_2_hash, &leaf_2).unwrap();
            store.put(&mut txn, &node_hash, &node).unwrap();

            // Commit the transaction.
            txn.commit().unwrap();
        }

        // Now let's check to see if the values were stored again
        {
            // Create a read transaction
            let txn = env.create_read_txn().unwrap();

            // Get the values in the store
            assert_eq!(Some(leaf_1), store.get(&txn, &leaf_1_hash).unwrap());
            assert_eq!(Some(leaf_2), store.get(&txn, &leaf_2_hash).unwrap());
            assert_eq!(Some(node), store.get(&txn, &node_hash).unwrap());

            // Commit the read transaction.
            txn.commit().unwrap();
        }
    }
}
