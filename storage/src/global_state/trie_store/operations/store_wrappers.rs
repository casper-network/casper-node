use std::marker::PhantomData;
#[cfg(debug_assertions)]
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest,
};

use crate::global_state::{store::Store, trie::Trie, trie_store::TrieStore};

/// A [`TrieStore`] wrapper that panics in debug mode whenever an attempt to deserialize [`V`] is
/// made, otherwise it behaves as a [`TrieStore`].
///
/// To ensure this wrapper has zero overhead, a debug assertion is used.
pub(crate) struct NonDeserializingStore<'a, K, V, S>(&'a S, PhantomData<*const (K, V)>)
where
    S: TrieStore<K, V>;

impl<'a, K, V, S> NonDeserializingStore<'a, K, V, S>
where
    S: TrieStore<K, V>,
{
    pub(crate) fn new(store: &'a S) -> Self {
        Self(store, PhantomData)
    }
}

impl<'a, K, V, S> Store<Digest, Trie<K, V>> for NonDeserializingStore<'a, K, V, S>
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
            let trie: Trie<K, V> = bytesrepr::deserialize_from_slice(bytes)?;
            if let Trie::Leaf { .. } = trie {
                panic!("Tried to deserialize a value but expected no deserialization to happen.")
            }
            Ok(trie)
        }
        #[cfg(not(debug_assertions))]
        {
            bytesrepr::deserialize_from_slice(bytes)
        }
    }
}

pub(crate) struct OnceDeserializingStore<'a, K: ToBytes, V: ToBytes, S: TrieStore<K, V>> {
    store: &'a S,
    #[cfg(debug_assertions)]
    deserialize_tracking: Arc<Mutex<HashSet<Digest>>>,
    _marker: PhantomData<*const (K, V)>,
}

impl<'a, K, V, S> OnceDeserializingStore<'a, K, V, S>
where
    K: ToBytes,
    V: ToBytes,
    S: TrieStore<K, V>,
{
    pub(crate) fn new(store: &'a S) -> Self {
        Self {
            store,
            #[cfg(debug_assertions)]
            deserialize_tracking: Arc::new(Mutex::new(HashSet::new())),
            _marker: PhantomData,
        }
    }
}

impl<'a, K, V, S> Store<Digest, Trie<K, V>> for OnceDeserializingStore<'a, K, V, S>
where
    K: ToBytes,
    V: ToBytes,
    S: TrieStore<K, V>,
{
    type Error = S::Error;

    type Handle = S::Handle;

    #[inline]
    fn handle(&self) -> Self::Handle {
        self.store.handle()
    }

    #[inline]
    fn deserialize_value(&self, bytes: &[u8]) -> Result<Trie<K, V>, bytesrepr::Error>
    where
        Trie<K, V>: FromBytes,
    {
        #[cfg(debug_assertions)]
        {
            let trie: Trie<K, V> = bytesrepr::deserialize_from_slice(bytes)?;
            if let Trie::Leaf { .. } = trie {
                let trie_hash = trie.trie_hash()?;
                let mut tracking = self.deserialize_tracking.lock().expect("Poisoned lock");
                if tracking.get(&trie_hash).is_some() {
                    panic!("Tried to deserialize a value more than once.");
                } else {
                    tracking.insert(trie_hash);
                }
            }
            Ok(trie)
        }
        #[cfg(not(debug_assertions))]
        {
            bytesrepr::deserialize_from_slice(bytes)
        }
    }
}
