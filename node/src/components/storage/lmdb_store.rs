use std::{fmt::Debug, marker::PhantomData, path::Path};

use lmdb::{
    self, Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction, WriteFlags,
};
use smallvec::smallvec;
use tracing::info;

use super::{Error, Multiple, Result, Store, Value};

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
    fn get_values(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V>>> {
        let mut serialized_ids = Multiple::new();
        for id in &ids {
            serialized_ids
                .push(bincode::serialize(id).map_err(|error| Error::from_serialization(*error)));
        }

        let mut values = smallvec![];
        let txn = self.env.begin_ro_txn().expect("should create ro txn");
        for id in serialized_ids {
            match id {
                Ok(id) => {
                    match txn.get(self.db, &id) {
                        Ok(serialized_value) => {
                            let value_result = bincode::deserialize(serialized_value)
                                .map(Some)
                                .map_err(|error| Error::from_deserialization(*error));
                            values.push(value_result)
                        }
                        Err(lmdb::Error::NotFound) => {
                            values.push(Ok(None));
                        }
                        Err(error) => panic!("should get: {:?}", error),
                    };
                }
                Err(error) => values.push(Err(error)),
            }
        }
        txn.commit().expect("should commit txn");
        values
    }
}

impl<V: Value> Store for LmdbStore<V> {
    type Value = V;

    fn put(&self, value: V) -> Result<bool> {
        let id =
            bincode::serialize(value.id()).map_err(|error| Error::from_serialization(*error))?;
        let serialized_value =
            bincode::serialize(&value).map_err(|error| Error::from_serialization(*error))?;
        let mut txn = self.env.begin_rw_txn().expect("should create rw txn");
        let result = match txn.put(self.db, &id, &serialized_value, WriteFlags::NO_OVERWRITE) {
            Ok(()) => true,
            Err(lmdb::Error::KeyExist) => false,
            Err(error) => panic!("should put: {:?}", error),
        };
        txn.commit().expect("should commit txn");
        Ok(result)
    }

    fn get(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V>>> {
        self.get_values(ids)
    }

    fn get_headers(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V::Header>>> {
        self.get_values(ids)
            .into_iter()
            .map(|value_result| value_result.map(|maybe_value| maybe_value.map(Value::take_header)))
            .collect()
    }

    fn ids(&self) -> Result<Vec<V::Id>> {
        let txn = self.env.begin_ro_txn().expect("should create ro txn");
        let mut ids = vec![];
        {
            let mut cursor = txn
                .open_ro_cursor(self.db)
                .expect("should create ro cursor");
            for (id, _value) in cursor.iter() {
                let id: V::Id = bincode::deserialize(id)
                    .map_err(|error| Error::from_deserialization(*error))?;
                ids.push(id);
            }
        }
        txn.commit().expect("should commit txn");
        Ok(ids)
    }
}
