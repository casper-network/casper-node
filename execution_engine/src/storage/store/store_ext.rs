//! Extension traits for store.

use casper_types::bytesrepr::{FromBytes, ToBytes};

use crate::storage::store::Store;

/// Extension trait for Store.
pub trait StoreExt<K, V>: Store<K, V> {
    /// Returns multiple optional values (each may exist or not) from the store in one transaction.
    fn get_many<'a>(&self, keys: impl Iterator<Item = &'a K>) -> Result<Vec<Option<V>>, Self::Error>
    where
        K: AsRef<[u8]> + 'a,
        V: FromBytes,
    {
        let mut ret: Vec<Option<V>> = Vec::new();
        for key in keys {
            let result = self.get(key)?;
            ret.push(result)
        }
        Ok(ret)
    }

    /// Puts multiple key/value pairs into the store in one transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put_many<'a>(&self, pairs: impl Iterator<Item = (&'a K, &'a V)>) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]> + 'a,
        V: ToBytes + 'a,
    {
        for (key, value) in pairs {
            self.put(key, value)?;
        }
        Ok(())
    }
}

impl<K, V, T: Store<K, V>> StoreExt<K, V> for T {}
