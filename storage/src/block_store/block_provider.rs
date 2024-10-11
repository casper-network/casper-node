use super::error::BlockStoreError;

/// A block store that supports read/write operations consistently.
pub trait BlockStoreProvider {
    /// Reader alias.
    type Reader<'a>: BlockStoreTransaction
    where
        Self: 'a;
    /// ReaderWriter alias.
    type ReaderWriter<'a>: BlockStoreTransaction
    where
        Self: 'a;

    /// Check out read only handle.
    fn checkout_ro(&self) -> Result<Self::Reader<'_>, BlockStoreError>;
    /// Check out read write handle.
    fn checkout_rw(&mut self) -> Result<Self::ReaderWriter<'_>, BlockStoreError>;
}

/// Block store transaction.
pub trait BlockStoreTransaction {
    /// Commit changes to the block store.
    fn commit(self) -> Result<(), BlockStoreError>;

    /// Roll back any temporary changes to the block store.
    fn rollback(self);
}

/// Data reader definition.
pub trait DataReader<K, T> {
    /// Read item at key.
    fn read(&self, key: K) -> Result<Option<T>, BlockStoreError>;
    /// Returns true if item exists at key, else false.
    fn exists(&self, key: K) -> Result<bool, BlockStoreError>;
}

/// Data write definition.
pub trait DataWriter<K, T> {
    /// Write item to store and return key.
    fn write(&mut self, data: &T) -> Result<K, BlockStoreError>;
    /// Delete item at key from store.
    fn delete(&mut self, key: K) -> Result<(), BlockStoreError>;
}
