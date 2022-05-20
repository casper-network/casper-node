mod store_ext;
#[cfg(test)]
pub(crate) mod tests;

use casper_types::bytesrepr::{self, Bytes, FromBytes, ToBytes};

pub use self::store_ext::StoreExt;

/// Base trait for db operations that can raise an error.
pub trait ErrorSource: Sized {
    /// Type of error held by the trait.
    type Error: From<bytesrepr::Error>;
}

/// Represents a store that can be read from.
pub trait BytesReader: ErrorSource {
    /// Reads the value from the corresponding key.
    fn read_bytes(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;
}

/// Represents a store that can be written to.
pub trait BytesWriter: ErrorSource {
    /// Writes a key-value pair.
    fn write_bytes(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;
}

/// Store is responsible for abstracting `get` and `put` operations over the underlying store
/// specified by its associated `Handle` type.
pub trait Store<K, V>: BytesReader + BytesWriter {
    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get(&self, key: &K) -> Result<Option<V>, Self::Error>
    where
        K: AsRef<[u8]>,
        V: FromBytes,
    {
        let raw = self.read_bytes(key.as_ref())?;
        match raw {
            Some(bytes) => {
                let value = bytesrepr::deserialize_from_slice(bytes)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Puts a `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put(&self, key: &K, value: &V) -> Result<(), Self::Error>
    where
        K: AsRef<[u8]>,
        V: ToBytes,
    {
        self.write_bytes(key.as_ref(), &value.to_bytes()?)
    }
}
