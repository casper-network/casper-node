use std::marker::PhantomData;

use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes};

use crate::storage::{store::Store, trie::Trie, trie_store::TrieStore};

/// A [`TrieStore`] wrapper that panics in debug mode whenever an attempt to deserialize [`V`] is
/// made, otherwise it behaves as a [`TrieStore`].
///
/// The debug panic is used to ensure that this wrapper has  To ensure this wrapper has zero
/// overhead, a debug assertion is used.
pub(crate) struct EnsureNeverDeserializes<'a, K, V, S>(&'a S, PhantomData<*const (K, V)>)
where
    S: TrieStore<K, V>;

impl<'a, K, V, S> EnsureNeverDeserializes<'a, K, V, S>
where
    S: TrieStore<K, V>,
{
    pub(crate) fn new(store: &'a S) -> Self {
        Self(store, PhantomData)
    }
}

impl<'a, K, V, S> Store<Digest, Trie<K, V>> for EnsureNeverDeserializes<'a, K, V, S>
where
    S: TrieStore<K, V>,
{
    type Error = S::Error;

    type Handle = S::Handle;

    #[inline]
    fn handle(&self) -> Self::Handle {
        self.0.handle()
    }

    #[inline]
    fn deserialize_value(&self, bytes: &[u8]) -> Result<Trie<K, V>, bytesrepr::Error>
    where
        Trie<K, V>: FromBytes,
    {
        #[cfg(debug_assertions)]
        {
            let _ = bytes;
            panic!("Tried to deserialize a value but expected no deserialization to happen.")
        }
        #[cfg(not(debug_assertions))]
        {
            bytesrepr::deserialize_from_slice(bytes)
        }
    }
}
