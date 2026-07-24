use datasize::DataSize;
use lmdb::{
    Cursor, Database, DatabaseFlags, Environment, RwCursor, RwTransaction,
    Transaction as LmdbTransaction,
};
use serde::de::DeserializeOwned;
#[cfg(test)]
use serde::Serialize;
use std::{collections::BTreeSet, marker::PhantomData};
use tracing::error;

use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    execution::ExecutionResult,
    Approval, BlockBody, BlockBodyV1, BlockHash, BlockHeader, BlockHeaderV1, BlockSignatures,
    BlockSignaturesV1, Deploy, DeployHash, Digest, Transaction, TransactionHash, TransferV1,
};

use super::{
    super::{
        error::BlockStoreError,
        types::{ApprovalsHashes, DeployMetadataV1, LegacyApprovalsHashes, Transfers},
        DbRawBytesSpec,
    },
    lmdb_ext::{self, LmdbExtError, TransactionExt, WriteTransactionExt},
};

pub(crate) trait VersionedKey: ToBytes {
    type Legacy: AsRef<[u8]>;

    fn legacy_key(&self) -> Option<&Self::Legacy>;
}

pub(crate) trait VersionedValue: ToBytes + FromBytes {
    type Legacy: 'static + DeserializeOwned + Into<Self>;
}

impl VersionedKey for TransactionHash {
    type Legacy = DeployHash;

    fn legacy_key(&self) -> Option<&Self::Legacy> {
        match self {
            TransactionHash::Deploy(deploy_hash) => Some(deploy_hash),
            TransactionHash::V1(_) => None,
            TransactionHash::Evm(_) => None,
        }
    }
}

impl VersionedKey for BlockHash {
    type Legacy = BlockHash;

    fn legacy_key(&self) -> Option<&Self::Legacy> {
        Some(self)
    }
}

impl VersionedKey for Digest {
    type Legacy = Digest;

    fn legacy_key(&self) -> Option<&Self::Legacy> {
        Some(self)
    }
}

impl VersionedValue for Transaction {
    type Legacy = Deploy;
}

impl VersionedValue for BlockHeader {
    type Legacy = BlockHeaderV1;
}

impl VersionedValue for BlockBody {
    type Legacy = BlockBodyV1;
}

impl VersionedValue for ApprovalsHashes {
    type Legacy = LegacyApprovalsHashes;
}

impl VersionedValue for ExecutionResult {
    type Legacy = DeployMetadataV1;
}

impl VersionedValue for BTreeSet<Approval> {
    type Legacy = BTreeSet<Approval>;
}

impl VersionedValue for BlockSignatures {
    type Legacy = BlockSignaturesV1;
}

impl VersionedValue for Transfers {
    type Legacy = Vec<TransferV1>;
}

/// A pair of databases, one holding the original legacy form of the data, and the other holding the
/// new versioned, future-proof form of the data.
///
/// Specific entries should generally not be repeated - they will either be held in the legacy or
/// the current DB, but not both.  Data is not migrated from legacy to current, but newly-stored
/// data will always be written to the current DB, even if it is of the type `V::Legacy`.
///
/// Exceptions to this can occur if a pre-existing legacy entry is re-stored, in which case there
/// will be a duplicated entry in the `legacy` and `current` DBs.  This should not be a common
/// occurrence though.
#[derive(Eq, PartialEq, DataSize, Debug)]
pub(crate) struct VersionedDatabases<K, V> {
    /// Legacy form of the data, with the key as `K::Legacy` type (converted to bytes using
    /// `AsRef<[u8]>`) and the value bincode-encoded.
    #[data_size(skip)]
    pub legacy: Database,
    /// Current form of the data, with the key as `K` bytesrepr-encoded and the value as `V` also
    /// bytesrepr-encoded.
    #[data_size(skip)]
    pub current: Database,
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> Clone for VersionedDatabases<K, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, V> Copy for VersionedDatabases<K, V> {}

/// Splits the keyspace into `num_partitions` partitions by first-byte prefix, returning each
/// partition's inclusive lower-bound byte (`bounds.len() == num_partitions`).
///
/// Callers should treat `bounds[i + 1]` as partition `i`'s exclusive upper bound, with the last
/// partition running to the true end of the database. This is only a reasonable partitioning
/// scheme for databases keyed by content-addressed hashes (e.g. `BlockHash`/`Digest`), since
/// those are close enough to uniformly distributed for the resulting partitions to be
/// similarly-sized; it says nothing useful about, say, sequential integer keys.
pub(super) fn partition_bounds(num_partitions: usize) -> Vec<u8> {
    let num_partitions = num_partitions.max(1);
    (0..num_partitions)
        .map(|i| ((i * 256) / num_partitions) as u8)
        .collect()
}

impl<K, V> VersionedDatabases<K, V>
where
    K: VersionedKey + std::fmt::Display,
    V: VersionedValue + 'static,
{
    pub(super) fn new(
        env: &Environment,
        legacy_name: &str,
        current_name: &str,
    ) -> Result<Self, lmdb::Error> {
        Ok(VersionedDatabases {
            legacy: env.create_db(Some(legacy_name), DatabaseFlags::empty())?,
            current: env.create_db(Some(current_name), DatabaseFlags::empty())?,
            _phantom: PhantomData,
        })
    }

    pub(super) fn put(
        &self,
        txn: &mut RwTransaction,
        key: &K,
        value: &V,
        overwrite: bool,
    ) -> Result<bool, LmdbExtError> {
        txn.put_value_bytesrepr(self.current, key, value, overwrite)
    }

    pub(super) fn get<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        key: &K,
    ) -> Result<Option<V>, LmdbExtError> {
        match txn.get_value_bytesrepr(self.current, key) {
            Ok(Some(value)) => return Ok(Some(value)),
            Ok(None) => {
                // check legacy db
            }
            Err(err) => {
                error!(%err, "versioned_database: failed to retrieve record from current db");
                return Err(err);
            }
        }

        let legacy_key = match key.legacy_key() {
            Some(key) => key,
            None => return Ok(None),
        };

        Ok(txn
            .get_value::<_, V::Legacy>(self.legacy, legacy_key)?
            .map(Into::into))
    }

    pub(super) fn get_raw<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        key: &[u8],
    ) -> Result<Option<DbRawBytesSpec>, LmdbExtError> {
        if key.is_empty() {
            return Ok(None);
        }
        let value = txn.get(self.current, &key);
        match value {
            Ok(raw_bytes) => Ok(Some(DbRawBytesSpec::new_current(raw_bytes))),
            Err(lmdb::Error::NotFound) => {
                let value = txn.get(self.legacy, &key);
                match value {
                    Ok(raw_bytes) => Ok(Some(DbRawBytesSpec::new_legacy(raw_bytes))),
                    Err(lmdb::Error::NotFound) => Ok(None),
                    Err(err) => Err(err.into()),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    pub(super) fn exists<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        key: &K,
    ) -> Result<bool, LmdbExtError> {
        if txn.value_exists_bytesrepr(self.current, key)? {
            return Ok(true);
        }

        let legacy_key = match key.legacy_key() {
            Some(key) => key,
            None => return Ok(false),
        };

        txn.value_exists(self.legacy, legacy_key)
    }

    /// Deletes the value under `key` from both the current and legacy DBs.
    ///
    /// Returns `Ok` if the value is successfully deleted from either or both the DBs, or if the
    /// value did not exist in either.
    pub(super) fn delete(&self, txn: &mut RwTransaction, key: &K) -> Result<(), LmdbExtError> {
        let serialized_key = lmdb_ext::serialize_bytesrepr(key)?;
        let current_result = match txn.del(self.current, &serialized_key, None) {
            Ok(_) | Err(lmdb::Error::NotFound) => Ok(()),
            Err(error) => Err(error.into()),
        };
        // Avoid returning early for the case where `current_result` is Ok, since some
        // `VersionedDatabases` could possibly have the same entry in both DBs.

        let legacy_key = match key.legacy_key() {
            Some(key) => key,
            None => return current_result,
        };

        let legacy_result = match txn.del(self.legacy, legacy_key, None) {
            Ok(_) | Err(lmdb::Error::NotFound) => Ok(()),
            Err(error) => Err(error.into()),
        };

        match (current_result, legacy_result) {
            (Err(error), _) => Err(error),
            (_, Err(error)) => Err(error),
            (Ok(_), Ok(_)) => Ok(()),
        }
    }

    /// Iterates every row in the current database, deserializing the value and calling `f` with the
    /// cursor, the row's raw (`bytesrepr`-encoded) key bytes, and the parsed value.
    ///
    /// Not currently called outside of tests: `rebuild_indexes` uses the read-only, partitioned
    /// `for_each_value_in_current_partition` instead. Kept (with its `RwCursor`, delete-capable
    /// signature) as ready-made infrastructure for a possible future delete-while-iterating use
    /// (e.g. pruning); remove if that doesn't materialize.
    #[allow(dead_code)]
    pub(super) fn for_each_value_in_current<'a, F>(
        &self,
        txn: &'a mut RwTransaction,
        f: &mut F,
    ) -> Result<(), BlockStoreError>
    where
        F: FnMut(&mut RwCursor<'a>, &[u8], V) -> Result<(), BlockStoreError>,
    {
        let mut cursor = txn
            .open_rw_cursor(self.current)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        for row in cursor.iter() {
            let (raw_key, raw_val) =
                row.map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            let value: V = lmdb_ext::deserialize_bytesrepr(raw_val)
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            f(&mut cursor, raw_key, value)?;
        }
        Ok(())
    }

    /// Iterates every row in the legacy database, deserializing the value and calling `f` with the
    /// cursor, the row's raw (`K::Legacy`'s `AsRef<[u8]>`) key bytes, and the parsed value.
    ///
    /// See the note on [`Self::for_each_value_in_current`]: unused outside tests since
    /// `rebuild_indexes` moved to the partitioned read-only scan.
    #[allow(dead_code)]
    pub(super) fn for_each_value_in_legacy<'a, F>(
        &self,
        txn: &'a mut RwTransaction,
        f: &mut F,
    ) -> Result<(), BlockStoreError>
    where
        F: FnMut(&mut RwCursor<'a>, &[u8], V) -> Result<(), BlockStoreError>,
    {
        let mut cursor = txn
            .open_rw_cursor(self.legacy)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        for row in cursor.iter() {
            let (raw_key, raw_val) =
                row.map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            let value: V::Legacy = lmdb_ext::deserialize(raw_val)
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            f(&mut cursor, raw_key, value.into())?;
        }
        Ok(())
    }

    /// Scans partition `partition_index` (of `bounds.len()` partitions, as computed by
    /// [`partition_bounds`]) of the current database using a read-only cursor, deserializing
    /// each value and calling `f` with the row's raw key bytes and the parsed value.
    ///
    /// Read-only transactions (unlike the `RwTransaction` used by
    /// [`Self::for_each_value_in_current`]) can be opened concurrently from multiple threads,
    /// which is the point of partitioning: each thread scans a disjoint slice of the keyspace
    /// through its own transaction.
    pub(super) fn for_each_value_in_current_partition<Tx, F>(
        &self,
        txn: &Tx,
        partition_index: usize,
        bounds: &[u8],
        f: &mut F,
    ) -> Result<(), BlockStoreError>
    where
        Tx: LmdbTransaction,
        F: FnMut(&[u8], V) -> Result<(), BlockStoreError>,
    {
        let mut cursor = txn
            .open_ro_cursor(self.current)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let upper_bound = bounds.get(partition_index + 1).copied();
        let iter = cursor.iter_from([bounds[partition_index]]);
        for row in iter {
            let (raw_key, raw_val) =
                row.map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            if upper_bound.is_some_and(|upper| raw_key.first().is_some_and(|&b| b >= upper)) {
                break;
            }
            let value: V = lmdb_ext::deserialize_bytesrepr(raw_val)
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            f(raw_key, value)?;
        }
        Ok(())
    }

    /// Scans partition `partition_index` of the legacy database using a read-only cursor; see
    /// [`Self::for_each_value_in_current_partition`].
    pub(super) fn for_each_value_in_legacy_partition<Tx, F>(
        &self,
        txn: &Tx,
        partition_index: usize,
        bounds: &[u8],
        f: &mut F,
    ) -> Result<(), BlockStoreError>
    where
        Tx: LmdbTransaction,
        F: FnMut(&[u8], V) -> Result<(), BlockStoreError>,
    {
        let mut cursor = txn
            .open_ro_cursor(self.legacy)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let upper_bound = bounds.get(partition_index + 1).copied();
        let iter = cursor.iter_from([bounds[partition_index]]);
        for row in iter {
            let (raw_key, raw_val) =
                row.map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            if upper_bound.is_some_and(|upper| raw_key.first().is_some_and(|&b| b >= upper)) {
                break;
            }
            let value: V::Legacy = lmdb_ext::deserialize(raw_val)
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            f(raw_key, value.into())?;
        }
        Ok(())
    }

    /// Writes to the `legacy` database.
    #[cfg(test)]
    pub(super) fn put_legacy(
        &self,
        txn: &mut RwTransaction,
        legacy_key: &K::Legacy,
        legacy_value: &V::Legacy,
        overwrite: bool,
    ) -> bool
    where
        V::Legacy: Serialize,
    {
        txn.put_value(self.legacy, legacy_key, legacy_value, overwrite)
            .expect("should put legacy value")
    }
}

#[cfg(test)]
mod tests {
    use crate::block_store::lmdb::lmdb_block_store::new_environment;
    use lmdb::WriteFlags;
    use std::collections::HashMap;

    use tempfile::TempDir;

    use casper_types::testing::TestRng;

    use super::*;

    struct Fixture {
        rng: TestRng,
        env: Environment,
        dbs: VersionedDatabases<TransactionHash, Transaction>,
        random_transactions: HashMap<TransactionHash, Transaction>,
        legacy_transactions: HashMap<DeployHash, Deploy>,
        _data_dir: TempDir,
    }

    impl Fixture {
        fn new() -> Fixture {
            let rng = TestRng::new();
            let data_dir = TempDir::new().expect("should create temp dir");
            let env = new_environment(1024 * 1024, data_dir.path()).unwrap();
            let dbs = VersionedDatabases::new(&env, "legacy", "current").unwrap();
            let mut fixture = Fixture {
                rng,
                env,
                dbs,
                random_transactions: HashMap::new(),
                legacy_transactions: HashMap::new(),
                _data_dir: data_dir,
            };
            for _ in 0..3 {
                let transaction = Transaction::random(&mut fixture.rng);
                assert!(fixture
                    .random_transactions
                    .insert(transaction.hash(), transaction)
                    .is_none());
                let deploy = Deploy::random(&mut fixture.rng);
                assert!(fixture
                    .legacy_transactions
                    .insert(*deploy.hash(), deploy)
                    .is_none());
            }
            fixture
        }
    }

    #[test]
    fn should_put() {
        let fixture = Fixture::new();
        let (transaction_hash, transaction) = fixture.random_transactions.iter().next().unwrap();

        // Should return `true` on first `put`.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        assert!(fixture
            .dbs
            .put(&mut txn, transaction_hash, transaction, true)
            .unwrap());

        // Should return `false` on duplicate `put` if not set to overwrite.
        assert!(!fixture
            .dbs
            .put(&mut txn, transaction_hash, transaction, false)
            .unwrap());

        // Should return `true` on duplicate `put` if set to overwrite.
        assert!(fixture
            .dbs
            .put(&mut txn, transaction_hash, transaction, true)
            .unwrap());
    }

    #[test]
    fn should_get() {
        let mut fixture = Fixture::new();
        let (transaction_hash, transaction) = fixture.random_transactions.iter().next().unwrap();
        let (deploy_hash, deploy) = fixture.legacy_transactions.iter().next().unwrap();

        // Inject the deploy into the legacy DB and store the random transaction in the current DB.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        assert!(fixture.dbs.put_legacy(&mut txn, deploy_hash, deploy, true));
        assert!(fixture
            .dbs
            .put(&mut txn, transaction_hash, transaction, true)
            .unwrap());
        txn.commit().unwrap();

        // Should get the deploy.
        let txn = fixture.env.begin_ro_txn().unwrap();
        assert_eq!(
            fixture
                .dbs
                .get(&txn, &TransactionHash::from(*deploy_hash))
                .unwrap(),
            Some(Transaction::from(deploy.clone()))
        );

        // Should get the random transaction.
        assert_eq!(
            fixture.dbs.get(&txn, transaction_hash).unwrap(),
            Some(transaction.clone())
        );

        // Should return `Ok(None)` for non-existent data.
        let random_hash = Transaction::random(&mut fixture.rng).hash();
        assert!(fixture.dbs.get(&txn, &random_hash).unwrap().is_none());
    }

    #[test]
    fn should_exist() {
        let mut fixture = Fixture::new();
        let (transaction_hash, transaction) = fixture.random_transactions.iter().next().unwrap();
        let (deploy_hash, deploy) = fixture.legacy_transactions.iter().next().unwrap();

        // Inject the deploy into the legacy DB and store the random transaction in the current DB.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        assert!(fixture.dbs.put_legacy(&mut txn, deploy_hash, deploy, true));
        assert!(fixture
            .dbs
            .put(&mut txn, transaction_hash, transaction, true)
            .unwrap());
        txn.commit().unwrap();

        // The deploy should exist.
        let txn = fixture.env.begin_ro_txn().unwrap();
        assert!(fixture
            .dbs
            .exists(&txn, &TransactionHash::from(*deploy_hash))
            .unwrap());

        // The random transaction should exist.
        assert!(fixture.dbs.exists(&txn, transaction_hash).unwrap());

        // Random data should not exist.
        let random_hash = Transaction::random(&mut fixture.rng).hash();
        assert!(!fixture.dbs.exists(&txn, &random_hash).unwrap());
    }

    #[test]
    fn should_delete() {
        let mut fixture = Fixture::new();
        let (transaction_hash, transaction) = fixture.random_transactions.iter().next().unwrap();
        let (deploy_hash, deploy) = fixture.legacy_transactions.iter().next().unwrap();

        // Inject the deploy into the legacy DB and store the random transaction in the current DB.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        assert!(fixture.dbs.put_legacy(&mut txn, deploy_hash, deploy, true));
        assert!(fixture
            .dbs
            .put(&mut txn, transaction_hash, transaction, true)
            .unwrap());
        // Also store the legacy deploy in the `current` DB.  While being an edge case, we still
        // need to ensure that deleting removes both copies of the deploy.
        assert!(fixture
            .dbs
            .put(
                &mut txn,
                &TransactionHash::from(*deploy_hash),
                &Transaction::from(deploy.clone()),
                true
            )
            .unwrap());
        txn.commit().unwrap();

        // Should delete the deploy.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        fixture
            .dbs
            .delete(&mut txn, &TransactionHash::from(*deploy_hash))
            .unwrap();
        assert!(!fixture
            .dbs
            .exists(&txn, &TransactionHash::from(*deploy_hash))
            .unwrap());

        // Should delete the random transaction.
        fixture.dbs.delete(&mut txn, transaction_hash).unwrap();
        assert!(!fixture.dbs.exists(&txn, transaction_hash).unwrap());

        // Should report success when attempting to delete non-existent data.
        let random_hash = Transaction::random(&mut fixture.rng).hash();
        fixture.dbs.delete(&mut txn, &random_hash).unwrap();
    }

    #[test]
    fn should_iterate_current() {
        let fixture = Fixture::new();

        // Store all random transactions.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        for (transaction_hash, transaction) in fixture.random_transactions.iter() {
            assert!(fixture
                .dbs
                .put(&mut txn, transaction_hash, transaction, true)
                .unwrap());
        }
        txn.commit().unwrap();

        // Iterate `current`, deleting each cursor entry and gathering the visited values in a map.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        let mut visited = HashMap::new();
        let mut visitor = |cursor: &mut RwCursor, _raw_key: &[u8], transaction: Transaction| {
            cursor.del(WriteFlags::empty()).unwrap();
            let _ = visited.insert(transaction.hash(), transaction);
            Ok(())
        };
        fixture
            .dbs
            .for_each_value_in_current(&mut txn, &mut visitor)
            .unwrap();
        txn.commit().unwrap();

        // Ensure all values were visited and the DB doesn't contain them any more.
        assert_eq!(visited, fixture.random_transactions);
        let txn = fixture.env.begin_ro_txn().unwrap();
        for transaction_hash in fixture.random_transactions.keys() {
            assert!(!fixture.dbs.exists(&txn, transaction_hash).unwrap());
        }

        // Ensure a second run is a no-op.
        let mut visitor = |_cursor: &mut RwCursor, _raw_key: &[u8], _transaction: Transaction| {
            panic!("should never get called");
        };
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        fixture
            .dbs
            .for_each_value_in_current(&mut txn, &mut visitor)
            .unwrap();
    }

    #[test]
    fn should_iterate_legacy() {
        let fixture = Fixture::new();

        // Store all legacy transactions.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        for (deploy_hash, deploy) in fixture.legacy_transactions.iter() {
            assert!(fixture.dbs.put_legacy(&mut txn, deploy_hash, deploy, true));
        }
        txn.commit().unwrap();

        // Iterate `legacy`, deleting each cursor entry and gathering the visited values in a map.
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        let mut visited = HashMap::new();
        let mut visitor = |cursor: &mut RwCursor, _raw_key: &[u8], transaction: Transaction| {
            cursor.del(WriteFlags::empty()).unwrap();
            match transaction {
                Transaction::Deploy(deploy) => {
                    let _ = visited.insert(*deploy.hash(), deploy);
                }
                Transaction::V1(_) => unreachable!(),
                Transaction::Evm(_) => unreachable!(),
            }
            Ok(())
        };
        fixture
            .dbs
            .for_each_value_in_legacy(&mut txn, &mut visitor)
            .unwrap();
        txn.commit().unwrap();

        // Ensure all values were visited and the DB doesn't contain them any more.
        assert_eq!(visited, fixture.legacy_transactions);
        let txn = fixture.env.begin_ro_txn().unwrap();
        for deploy_hash in fixture.legacy_transactions.keys() {
            assert!(!fixture
                .dbs
                .exists(&txn, &TransactionHash::from(*deploy_hash))
                .unwrap());
        }

        // Ensure a second run is a no-op.
        let mut visitor = |_cursor: &mut RwCursor, _raw_key: &[u8], _transaction: Transaction| {
            panic!("should never get called");
        };
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        fixture
            .dbs
            .for_each_value_in_legacy(&mut txn, &mut visitor)
            .unwrap();
    }

    #[test]
    fn should_get_on_empty_key() {
        let fixture = Fixture::new();
        let txn = fixture.env.begin_ro_txn().unwrap();
        let key = vec![];
        let res = fixture.dbs.get_raw(&txn, &key);
        assert!(matches!(res, Ok(None)));
    }
}
