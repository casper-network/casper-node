use casperlabs_types::bytesrepr::{FromBytes, ToBytes};

use crate::components::contract_runtime::storage::{
    store::Store,
    transaction_source::{Readable, Writable},
};

pub trait StoreExt<K, V>: Store<K, V> {
    fn get_many<'a, T>(
        &self,
        txn: &T,
        keys: impl Iterator<Item = &'a K>,
    ) -> Result<Vec<Option<V>>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        K: ToBytes + 'a,
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

    fn put_many<'a, T>(
        &self,
        txn: &mut T,
        pairs: impl Iterator<Item = (&'a K, &'a V)>,
    ) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        K: ToBytes + 'a,
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
