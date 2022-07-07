//! Extension traits for store.

use casper_types::bytesrepr::{FromBytes, ToBytes};

use crate::storage::{
    store::Store,
    transaction_source::{Readable, Writable},
};

/// Extension trait for Store.
pub trait StoreExt<K, V>: Store<K, V> {
    /// Returns multiple optional values (each may exist or not) from the store in one transaction.
    fn get_many<'a, T>(
        &self,
        txn: &T,
        keys: impl Iterator<Item = &'a K>,
    ) -> Result<Vec<Option<V>>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        K: AsRef<[u8]> + 'a,
        V: FromBytes,
        Self::Error: From<T::Error>,
    {
        let mut ret: Vec<Option<V>> = Vec::new();
        for key in keys {
            let result = self.get(txn, key)?;
            ret.push(result)
        }
        Ok(ret)
    }

    /// Puts multiple key/value pairs into the store in one transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put_many<'a, T>(
        &self,
        txn: &mut T,
        pairs: impl Iterator<Item = (&'a K, &'a V)>,
    ) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        K: AsRef<[u8]> + 'a,
        V: ToBytes + 'a,
        Self::Error: From<T::Error>,
    {
        for (key, value) in pairs {
            self.put(txn, key, value)?;
        }
        Ok(())
    }
}

impl<K, V, T: Store<K, V>> StoreExt<K, V> for T {}
