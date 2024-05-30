use super::error::BlockStoreError;

/// A block store that supports read/write operations consistently.
pub trait BlockStoreProvider {
    type Reader<'a>: BlockStoreTransaction
    where
        Self: 'a;
    type ReaderWriter<'a>: BlockStoreTransaction
    where
        Self: 'a;

    fn checkout_ro(&self) -> Result<Self::Reader<'_>, BlockStoreError>;
    fn checkout_rw(&mut self) -> Result<Self::ReaderWriter<'_>, BlockStoreError>;
}

pub trait BlockStoreTransaction {
    /// Commit changes to the block store.
    fn commit(self) -> Result<(), BlockStoreError>;

    /// Roll back any temporary changes to the block store.
    fn rollback(self);
}

pub trait DataReader<K, T> {
    fn read(&self, key: K) -> Result<Option<T>, BlockStoreError>;
    fn exists(&self, key: K) -> Result<bool, BlockStoreError>;
}

pub trait DataWriter<K, T> {
    fn write(&mut self, data: &T) -> Result<K, BlockStoreError>;
    fn delete(&mut self, key: K) -> Result<(), BlockStoreError>;
}
