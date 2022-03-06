mod store_ext;
#[cfg(test)]
pub(crate) mod tests;

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

pub use self::store_ext::StoreExt;
use crate::storage::transaction_source::{Readable, Writable};

/// Store is responsible for abstracting `get` and `put` operations over the underlying store
/// specified by its associated `Handle` type.
pub trait Store<K, V> {
    /// Errors possible from this store.
    type Error: From<bytesrepr::Error>;

    /// Underlying store type.
    type Handle;

    /// `handle` returns the underlying store.
    fn handle(&self) -> Self::Handle;

    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get<T>(&self, txn: &T, key: &K) -> Result<Option<V>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        K: AsRef<[u8]>,
        V: FromBytes,
        Self::Error: From<T::Error>,
    {
        let raw = self.get_raw(txn, key)?;
        match raw {
            Some(bytes) => {
                let value = bytesrepr::deserialize_from_slice(bytes)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get_raw<'a, T>(&self, txn: &'a T, key: &K) -> Result<Option<&'a [u8]>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        K: AsRef<[u8]>,
        Self::Error: From<T::Error>,
    {
        let handle = self.handle();
        Ok(txn.read(handle, key.as_ref())?)
    }

    /// Puts a `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put<T>(&self, txn: &mut T, key: &K, value: &V) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        K: AsRef<[u8]>,
        V: ToBytes,
        Self::Error: From<T::Error>,
    {
        self.put_raw(txn, key, &value.to_bytes()?)
    }

    /// Puts a raw `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put_raw<T>(&self, txn: &mut T, key: &K, trie_bytes: &[u8]) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        K: AsRef<[u8]>,
        Self::Error: From<T::Error>,
    {
        let handle = self.handle();
        txn.write(handle, key.as_ref(), trie_bytes)
            .map_err(Into::into)
    }
}
