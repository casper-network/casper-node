mod store_ext;
#[cfg(test)]
pub(crate) mod tests;

use casper_types::bytesrepr::{self, Bytes, FromBytes, ToBytes};

pub use self::store_ext::StoreExt;
use crate::storage::transaction_source::{Readable, Writable};

/// Store is responsible for abstracting `get` and `put` operations over the underlying store
/// specified by its associated `Handle` type.
pub trait Store<K, V>: Readable + Writable {
    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get(&self, key: &K) -> Result<Option<V>, Self::Error>
    where
        K: AsRef<[u8]>,
        V: FromBytes,
    {
        let raw = self.get_raw(key)?;
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
    fn get_raw(&self, key: &K) -> Result<Option<Bytes>, Self::Error>
    where
        K: AsRef<[u8]>,
    {
        self.read(key.as_ref())
    }

    /// Puts a `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put(&self, key: &K, value: &V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: ToBytes,
    {
        self.put_raw(key, &value.to_bytes()?)
    }

    /// Puts a raw `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put_raw(&self, key: &K, trie_bytes: &[u8]) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
    {
        self.write(key.as_ref(), trie_bytes).map_err(Into::into)
    }
}
