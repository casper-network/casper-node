use std::{fmt::Debug, marker::PhantomData, path::Path};

use lmdb::{
    self, Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction, WriteFlags,
};
use tracing::info;

use super::{Error, Result, Store, Value};

/// LMDB version of a store.
#[derive(Debug)]
pub(super) struct LmdbStore<V: Value> {
    env: Environment,
    db: Database,
    _phantom: PhantomData<V>,
}

impl<V: Value> LmdbStore<V> {
    pub(crate) fn new<P: AsRef<Path>>(db_path: P, max_size: usize) -> Result<Self> {
        let env = Environment::new()
            .set_flags(EnvironmentFlags::NO_SUB_DIR)
            .set_map_size(max_size)
            .open(db_path.as_ref())?;
        let db = env.create_db(None, DatabaseFlags::empty())?;
        info!("opened DB at {}", db_path.as_ref().display());

        Ok(LmdbStore {
            env,
            db,
            _phantom: PhantomData,
        })
    }
}

impl<V: Value> LmdbStore<V> {
    fn get_value(&self, id: &V::Id) -> Result<V> {
        let id = bincode::serialize(id).map_err(|error| Error::from_serialization(*error))?;
        let txn = self.env.begin_ro_txn()?;
        let serialized_value = match txn.get(self.db, &id) {
            Ok(value) => value,
            Err(lmdb::Error::NotFound) => return Err(Error::NotFound),
            Err(error) => return Err(error.into()),
        };
        let value = bincode::deserialize(serialized_value)
            .map_err(|error| Error::from_deserialization(*error))?;
        txn.commit()?;
        Ok(value)
    }
}

impl<V: Value> Store for LmdbStore<V> {
    type Value = V;

    fn put(&self, value: V) -> Result<()> {
        let id =
            bincode::serialize(value.id()).map_err(|error| Error::from_serialization(*error))?;
        let serialized_value =
            bincode::serialize(&value).map_err(|error| Error::from_serialization(*error))?;
        let mut txn = self.env.begin_rw_txn()?;
        txn.put(self.db, &id, &serialized_value, WriteFlags::empty())?;
        txn.commit()?;
        Ok(())
    }

    fn get(&self, id: &V::Id) -> Result<V> {
        self.get_value(id)
    }

    fn get_header(&self, id: &V::Id) -> Result<V::Header> {
        self.get_value(id).map(|value| value.take_header())
    }

    fn ids(&self) -> Result<Vec<V::Id>> {
        let txn = self.env.begin_ro_txn()?;
        let mut ids = vec![];
        {
            let mut cursor = txn.open_ro_cursor(self.db)?;
            for (id, _value) in cursor.iter() {
                let id: V::Id = bincode::deserialize(id)
                    .map_err(|error| Error::from_deserialization(*error))?;
                ids.push(id);
            }
        }
        txn.commit()?;
        Ok(ids)
    }
}
