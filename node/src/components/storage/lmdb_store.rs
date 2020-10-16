use std::{fmt::Debug, marker::PhantomData, path::Path};

use datasize::DataSize;
use lmdb::{
    self, Cursor, Database, DatabaseFlags, Environment, EnvironmentFlags, Transaction, WriteFlags,
};
use smallvec::smallvec;
use tracing::info;

use super::{
    DeployMetadata, DeployStore, Error, Event, Multiple, Result, StorageType, Store, Value,
    WithBlockHeight,
};
use crate::{
    effect::Effects,
    types::{json_compatibility::ExecutionResult, Item},
};

/// Used to namespace metadata associated with stored values.
#[repr(u8)]
enum Tag {
    DeployMetadata,
}

/// LMDB version of a store.
#[derive(DataSize, Debug)]
pub struct LmdbStore<V, M>
where
    V: Value,
{
    #[data_size(skip)] // Just a pointer to an external C lib
    env: Environment,
    #[data_size(skip)] // Just a pointer to an external C lib
    db: Database,
    _phantom: PhantomData<(V, M)>,
}

impl<V, M> LmdbStore<V, M>
where
    V: Value,
    M: Default + Send + Sync,
    V: Value + WithBlockHeight + 'static,
    M: Value + Item + 'static,
{
    pub(crate) fn new<P: AsRef<Path>>(
        db_path: P,
        max_size: usize,
    ) -> Result<(Self, Effects<Event<Self>>)> {
        let env = Environment::new()
            .set_flags(EnvironmentFlags::NO_SUB_DIR)
            .set_map_size(max_size)
            .open(db_path.as_ref())?;
        let db = env.create_db(None, DatabaseFlags::empty())?;
        info!("opened DB at {}", db_path.as_ref().display());

        Ok((
            LmdbStore {
                env,
                db,
                _phantom: PhantomData,
            },
            Effects::new(),
        ))
    }
}

impl<V: Value, M> LmdbStore<V, M> {
    fn get_values(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V>>> {
        let mut serialized_ids = Multiple::new();
        for id in &ids {
            serialized_ids.push(Self::serialized_id(id, None).map_err(Error::from));
        }

        let mut values = smallvec![];
        let txn = self.env.begin_ro_txn().expect("should create ro txn");
        for maybe_serialized_id in serialized_ids {
            match maybe_serialized_id {
                Ok(serialized_id) => {
                    match txn.get(self.db, &serialized_id) {
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

    fn serialized_id(id: &V::Id, maybe_tag: Option<Tag>) -> Result<Vec<u8>> {
        match maybe_tag {
            Some(tag) => bincode::serialize(&(tag as u8, id)),
            None => bincode::serialize(id),
        }
        .map_err(|error| Error::from_serialization(*error))
    }
}

impl<V: Value, M: Send + Sync> Store for LmdbStore<V, M> {
    type Value = V;

    fn put(&self, value: V) -> Result<bool> {
        let serialized_id = Self::serialized_id(value.id(), None)?;
        let serialized_value =
            bincode::serialize(&value).map_err(|error| Error::from_serialization(*error))?;
        let mut txn = self.env.begin_rw_txn().expect("should create rw txn");
        let result = match txn.put(
            self.db,
            &serialized_id,
            &serialized_value,
            // TODO - this should be changed back to `WriteFlags::NO_OVERWRITE` once the mutable
            //        data (i.e. blocks' proofs) are handled via metadata as per deploys'
            //        execution results.
            WriteFlags::default(),
        ) {
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
            for (serialized_id, _value) in cursor.iter() {
                if let Ok(id) = bincode::deserialize::<V::Id>(serialized_id) {
                    ids.push(id);
                }
            }
        }
        txn.commit().expect("should commit txn");
        Ok(ids)
    }
}

impl<D: Value, B: Value> DeployStore for LmdbStore<D, DeployMetadata<B>> {
    type Block = B;
    type Deploy = D;

    fn put_execution_result(
        &self,
        id: D::Id,
        block_hash: B::Id,
        execution_result: ExecutionResult,
    ) -> Result<bool> {
        // Get existing metadata associated with this deploy.
        let serialized_id = Self::serialized_id(&id, Some(Tag::DeployMetadata))?;
        let mut txn = self.env.begin_rw_txn().expect("should create rw txn");

        let mut metadata: DeployMetadata<B> = match txn.get(self.db, &serialized_id) {
            Ok(serialized_value) => bincode::deserialize(serialized_value)
                .map_err(|error| Error::from_deserialization(*error))?,
            Err(lmdb::Error::NotFound) => DeployMetadata::default(),
            Err(error) => panic!("should get: {:?}", error),
        };

        // If we already have this execution result, return false.
        if metadata
            .execution_results
            .insert(block_hash, execution_result)
            .is_some()
        {
            txn.commit().expect("should commit txn");
            return Ok(false);
        }

        // Store the updated metadata.
        let serialized_value =
            bincode::serialize(&metadata).map_err(|error| Error::from_serialization(*error))?;
        txn.put(
            self.db,
            &serialized_id,
            &serialized_value,
            WriteFlags::default(),
        )?;
        txn.commit().expect("should commit txn");
        Ok(true)
    }

    fn get_deploy_and_metadata(&self, id: D::Id) -> Result<Option<(D, DeployMetadata<B>)>> {
        let serialized_deploy_id = Self::serialized_id(&id, None)?;
        let serialized_metadata_id = Self::serialized_id(&id, Some(Tag::DeployMetadata))?;

        // Get the deploy.
        let txn = self.env.begin_ro_txn().expect("should create ro txn");
        let deploy: D = match txn.get(self.db, &serialized_deploy_id) {
            Ok(serialized_value) => bincode::deserialize(serialized_value)
                .map_err(|error| Error::from_deserialization(*error))?,
            Err(lmdb::Error::NotFound) => {
                // Return `None` if the deploy doesn't exist.
                txn.commit().expect("should commit txn");
                return Ok(None);
            }
            Err(error) => panic!("should get: {:?}", error),
        };

        // Get the metadata or create a default one.
        let metadata: DeployMetadata<B> = match txn.get(self.db, &serialized_metadata_id) {
            Ok(serialized_value) => bincode::deserialize(serialized_value)
                .map_err(|error| Error::from_deserialization(*error))?,
            Err(lmdb::Error::NotFound) => DeployMetadata::default(),
            Err(error) => panic!("should get: {:?}", error),
        };

        txn.commit().expect("should commit txn");
        Ok(Some((deploy, metadata)))
    }
}
