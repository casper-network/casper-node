mod store_ext;
#[cfg(test)]
pub(crate) mod tests;

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

pub use self::store_ext::StoreExt;
use crate::storage::transaction_source::{Readable, Writable};

pub trait Store<K, V> {
    type Error: From<bytesrepr::Error>;

    type Handle;

    fn handle(&self) -> Self::Handle;

    fn deserialize_hook(&self, _key: &K, bytes: Vec<u8>) -> Result<V, bytesrepr::Error>
    where
        V: FromBytes,
    {
        bytesrepr::deserialize(bytes)
    }

    fn get<T>(&self, txn: &T, key: &K) -> Result<Option<V>, Self::Error>
    where
        T: Readable<Handle = Self::Handle>,
        K: ToBytes,
        V: FromBytes,
        Self::Error: From<T::Error>,
    {
        let handle = self.handle();
        match txn.read(handle, &key.to_bytes()?)? {
            None => Ok(None),
            Some(value_bytes) => {
                let value = self.deserialize_hook(key, value_bytes.into())?;
                Ok(Some(value))
            }
        }
    }

    fn put<T>(&self, txn: &mut T, key: &K, value: &V) -> Result<(), Self::Error>
    where
        T: Writable<Handle = Self::Handle>,
        K: ToBytes,
        V: ToBytes,
        Self::Error: From<T::Error>,
    {
        let handle = self.handle();
        txn.write(handle, &key.to_bytes()?, &value.to_bytes()?)
            .map_err(Into::into)
    }
}
