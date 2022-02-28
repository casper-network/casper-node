use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use casper_types::bytesrepr::Bytes;

use crate::storage::{
    error::in_memory::Error,
    transaction_source::{Readable, Transaction, TransactionSource, Writable},
};

/// A marker for use in a mutex which represents the capability to perform a
/// write transaction.
struct WriteCapability;

type WriteLock<'a> = MutexGuard<'a, WriteCapability>;

type BytesMap = HashMap<Bytes, Bytes>;

#[cfg(test)]
type PoisonError<'a> = std::sync::PoisonError<MutexGuard<'a, HashMap<Option<String>, BytesMap>>>;

/// A read transaction for the in-memory trie store.
pub struct InMemoryReadTransaction {
    view: HashMap<Option<String>, BytesMap>,
}

impl InMemoryReadTransaction {
    pub(crate) fn new(store: &InMemoryEnvironment) -> Result<InMemoryReadTransaction, Error> {
        let view = {
            let db_ref = Arc::clone(&store.data);
            let view_lock = db_ref.lock()?;
            view_lock.to_owned()
        };
        Ok(InMemoryReadTransaction { view })
    }
}

impl Transaction for InMemoryReadTransaction {
    type Error = Error;

    type Handle = Option<String>;

    fn commit(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Readable for InMemoryReadTransaction {
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<&[u8]>, Self::Error> {
        let sub_view = match self.view.get(&handle) {
            Some(view) => view,
            None => return Ok(None),
        };
        Ok(sub_view.get(&Bytes::from(key)).map(Bytes::as_slice))
    }
}

/// A read-write transaction for the in-memory trie store.
pub struct InMemoryReadWriteTransaction<'a> {
    view: HashMap<Option<String>, BytesMap>,
    store_ref: Arc<Mutex<HashMap<Option<String>, BytesMap>>>,
    _write_lock: WriteLock<'a>,
}

impl<'a> InMemoryReadWriteTransaction<'a> {
    pub(crate) fn new(
        store: &'a InMemoryEnvironment,
    ) -> Result<InMemoryReadWriteTransaction<'a>, Error> {
        let store_ref = Arc::clone(&store.data);
        let view = {
            let view_lock = store_ref.lock()?;
            view_lock.to_owned()
        };
        let _write_lock = store.write_mutex.lock()?;
        Ok(InMemoryReadWriteTransaction {
            view,
            store_ref,
            _write_lock,
        })
    }
}

impl<'a> Transaction for InMemoryReadWriteTransaction<'a> {
    type Error = Error;

    type Handle = Option<String>;

    fn commit(self) -> Result<(), Self::Error> {
        let mut store_ref_lock = self.store_ref.lock()?;
        store_ref_lock.extend(self.view);
        Ok(())
    }
}

impl<'a> Readable for InMemoryReadWriteTransaction<'a> {
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<&[u8]>, Self::Error> {
        let sub_view = match self.view.get(&handle) {
            Some(view) => view,
            None => return Ok(None),
        };
        Ok(sub_view.get(&Bytes::from(key)).map(Bytes::as_slice))
    }
}

impl<'a> Writable for InMemoryReadWriteTransaction<'a> {
    fn write(&mut self, handle: Self::Handle, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        let sub_view = self.view.entry(handle).or_default();
        sub_view.insert(Bytes::from(key), Bytes::from(value));
        Ok(())
    }
}

/// An environment for the in-memory trie store.
pub struct InMemoryEnvironment {
    data: Arc<Mutex<HashMap<Option<String>, BytesMap>>>,
    write_mutex: Arc<Mutex<WriteCapability>>,
}

impl Default for InMemoryEnvironment {
    fn default() -> Self {
        let data = {
            let mut initial_map = HashMap::new();
            initial_map.insert(None, Default::default());
            Arc::new(Mutex::new(initial_map))
        };
        let write_mutex = Arc::new(Mutex::new(WriteCapability));
        InMemoryEnvironment { data, write_mutex }
    }
}

impl InMemoryEnvironment {
    /// Default constructor for `InMemoryEnvironment`.
    pub fn new() -> Self {
        Default::default()
    }

    #[cfg(test)]
    pub fn data(&self, name: Option<&str>) -> Result<Option<BytesMap>, PoisonError> {
        let data = self.data.lock()?;
        let name = name.map(ToString::to_string);
        let ret = data.get(&name).cloned();
        Ok(ret)
    }
}

impl<'a> TransactionSource<'a> for InMemoryEnvironment {
    type Error = Error;

    type Handle = Option<String>;

    type ReadTransaction = InMemoryReadTransaction;

    type ReadWriteTransaction = InMemoryReadWriteTransaction<'a>;

    fn create_read_txn(&'a self) -> Result<InMemoryReadTransaction, Self::Error> {
        InMemoryReadTransaction::new(self).map_err(Into::into)
    }

    fn create_read_write_txn(&'a self) -> Result<InMemoryReadWriteTransaction<'a>, Self::Error> {
        InMemoryReadWriteTransaction::new(self).map_err(Into::into)
    }
}
