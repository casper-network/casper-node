use crate::shared::newtypes::Blake2bHash;

/// Result from inserting a [Trie][storage::trie::Trie] into the trie store.  Contains:
///
/// - The trie key the inserted [Trie][storage::trie::Trie] (which is its [Blake2bHash])
///
/// - The missing transitively referred to descendant keys of the [Trie][storage::trie::Trie] that
///   was just inserted. Found using [storage::global_state::StateProvider::missing_trie_keys].
#[derive(Debug)]
pub struct InsertedTrieKeyAndMissingDescendants {
    inserted_trie_key: Blake2bHash,
    missing_descendant_trie_keys: Vec<Blake2bHash>,
}

impl InsertedTrieKeyAndMissingDescendants {
    pub(crate) fn new(
        inserted_trie_key: Blake2bHash,
        missing_descendant_trie_keys: Vec<Blake2bHash>,
    ) -> Self {
        InsertedTrieKeyAndMissingDescendants {
            inserted_trie_key,
            missing_descendant_trie_keys,
        }
    }

    pub fn inserted_trie_key(&self) -> Blake2bHash {
        self.inserted_trie_key
    }

    pub fn into_missing_descendant_trie_keys(self) -> Vec<Blake2bHash> {
        self.missing_descendant_trie_keys
    }
}
