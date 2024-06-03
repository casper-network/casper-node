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

use crate::global_state::{
    store::Store,
    transaction_source::{Readable, Writable},
    trie::Trie,
    trie_store::TrieStore,
};

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
            let trie: Trie<K, V> = self.0.deserialize_value(bytes)?;
            if let Trie::Leaf { .. } = trie {
                panic!("Tried to deserialize a value but expected no deserialization to happen.")
            }
            Ok(trie)
        }
        #[cfg(not(debug_assertions))]
        {
            self.0.deserialize_value(bytes)
        }
    }

    #[inline]
    fn serialize_value(&self, value: &Trie<K, V>) -> Result<Vec<u8>, bytesrepr::Error>
    where
        Trie<K, V>: ToBytes,
    {
        self.0.serialize_value(value)
    }

    #[inline]
    fn get<T>(&self, txn: &T, key: &Digest) -> Result<Option<Trie<K, V>>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Trie<K, V>: FromBytes,
        Self::Error: From<T::Error>,
    {
        self.0.get(txn, key)
    }

    #[inline]
    fn get_raw<T>(&self, txn: &T, key: &Digest) -> Result<Option<bytesrepr::Bytes>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Self::Error: From<T::Error>,
    {
        self.0.get_raw(txn, key)
    }

    #[inline]
    fn put<T>(&self, txn: &mut T, key: &Digest, value: &Trie<K, V>) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Trie<K, V>: ToBytes,
        Self::Error: From<T::Error>,
    {
        self.0.put(txn, key, value)
    }

    #[inline]
    fn put_raw<T>(
        &self,
        txn: &mut T,
        key: &Digest,
        value_bytes: std::borrow::Cow<'_, [u8]>,
    ) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Self::Error: From<T::Error>,
    {
        self.0.put_raw(txn, key, value_bytes)
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
            let trie: Trie<K, V> = self.store.deserialize_value(bytes)?;
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
            self.store.deserialize_value(bytes)
        }
    }

    #[inline]
    fn serialize_value(&self, value: &Trie<K, V>) -> Result<Vec<u8>, bytesrepr::Error>
    where
        Trie<K, V>: ToBytes,
    {
        self.store.serialize_value(value)
    }

    #[inline]
    fn get<T>(&self, txn: &T, key: &Digest) -> Result<Option<Trie<K, V>>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Trie<K, V>: FromBytes,
        Self::Error: From<T::Error>,
    {
        self.store.get(txn, key)
    }

    #[inline]
    fn get_raw<T>(&self, txn: &T, key: &Digest) -> Result<Option<bytesrepr::Bytes>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Self::Error: From<T::Error>,
    {
        self.store.get_raw(txn, key)
    }

    #[inline]
    fn put<T>(&self, txn: &mut T, key: &Digest, value: &Trie<K, V>) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Trie<K, V>: ToBytes,
        Self::Error: From<T::Error>,
    {
        self.store.put(txn, key, value)
    }

    #[inline]
    fn put_raw<T>(
        &self,
        txn: &mut T,
        key: &Digest,
        value_bytes: std::borrow::Cow<'_, [u8]>,
    ) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        Digest: AsRef<[u8]>,
        Self::Error: From<T::Error>,
    {
        self.store.put_raw(txn, key, value_bytes)
    }
}
