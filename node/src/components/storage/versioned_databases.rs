#[cfg(test)]
use std::{cmp::Ord, collections::BTreeSet};

use datasize::DataSize;
use lmdb::{
    Cursor, Database, DatabaseFlags, Environment, RoTransaction, RwCursor, RwTransaction,
    Transaction as LmdbTransaction,
};
use serde::de::DeserializeOwned;
#[cfg(test)]
use serde::Serialize;
use std::marker::PhantomData;

#[cfg(test)]
use casper_types::bytesrepr;
use casper_types::{
    binary_port::DbRawBytesSpec,
    bytesrepr::{FromBytes, ToBytes},
    execution::ExecutionResult,
    BlockBody, BlockBodyV1, BlockHash, BlockHeader, BlockHeaderV1, BlockSignatures,
    BlockSignaturesV1, Deploy, DeployHash, Digest, FinalizedApprovals, FinalizedDeployApprovals,
    Transaction, TransactionHash,
};

use super::{
    lmdb_ext::{self, LmdbExtError, TransactionExt, WriteTransactionExt},
    DeployMetadataV1, FatalStorageError, LegacyApprovalsHashes, RawDataAccess,
};
use crate::types::ApprovalsHashes;

pub(super) trait VersionedKey: ToBytes {
    type Legacy: AsRef<[u8]>;

    fn legacy_key(&self) -> Option<&Self::Legacy>;
}

pub(super) trait VersionedValue: ToBytes + FromBytes {
    type Legacy: 'static + DeserializeOwned + Into<Self>;
}

impl VersionedKey for TransactionHash {
    type Legacy = DeployHash;

    fn legacy_key(&self) -> Option<&Self::Legacy> {
        match self {
            TransactionHash::Deploy(deploy_hash) => Some(deploy_hash),
            TransactionHash::V1(_) => None,
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

impl VersionedValue for FinalizedApprovals {
    type Legacy = FinalizedDeployApprovals;
}

impl VersionedValue for BlockSignatures {
    type Legacy = BlockSignaturesV1;
}

impl<K, V> RawDataAccess for VersionedDatabases<K, V>
where
    K: VersionedKey,
    V: VersionedValue,
{
    fn get_raw_bytes(
        &self,
        txn: &RoTransaction,
        key: &[u8],
    ) -> Result<Option<DbRawBytesSpec>, LmdbExtError> {
        self.get_raw(txn, key)
    }
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
pub(super) struct VersionedDatabases<K, V> {
    /// Legacy form of the data, with the key as `K::Legacy` type (converted to bytes using
    /// `AsRef<[u8]>`) and the value bincode-encoded.
    #[data_size(skip)]
    legacy: Database,
    /// Current form of the data, with the key as `K` bytesrepr-encoded and the value as `V` also
    /// bytesrepr-encoded.
    #[data_size(skip)]
    current: Database,
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> Clone for VersionedDatabases<K, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, V> Copy for VersionedDatabases<K, V> {}

impl<K, V> VersionedDatabases<K, V>
where
    K: VersionedKey,
    V: VersionedValue,
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
        txn: &mut Tx,
        key: &K,
    ) -> Result<Option<V>, LmdbExtError> {
        if let Some(value) = txn.get_value_bytesrepr(self.current, key)? {
            return Ok(Some(value));
        }

        let legacy_key = match key.legacy_key() {
            Some(key) => key,
            None => return Ok(None),
        };

        Ok(txn
            .get_value::<_, V::Legacy>(self.legacy, legacy_key)?
            .map(Into::into))
    }

    pub(super) fn get_raw(
        &self,
        txn: &RoTransaction,
        key: &[u8],
    ) -> Result<Option<DbRawBytesSpec>, LmdbExtError> {
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
        txn: &mut Tx,
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
    /// cursor and the parsed value.
    pub(super) fn for_each_value_in_current<'a, F>(
        &self,
        txn: &'a mut RwTransaction,
        f: &mut F,
    ) -> Result<(), FatalStorageError>
    where
        F: FnMut(&mut RwCursor<'a>, V) -> Result<(), FatalStorageError>,
    {
        let mut cursor = txn.open_rw_cursor(self.current)?;
        for row in cursor.iter() {
            let (_, raw_val) = row?;
            let value: V = lmdb_ext::deserialize_bytesrepr(raw_val)?;
            f(&mut cursor, value)?;
        }
        Ok(())
    }

    /// Iterates every row in the legacy database, deserializing the value and calling `f` with the
    /// cursor and the parsed value.
    pub(super) fn for_each_value_in_legacy<'a, F>(
        &self,
        txn: &'a mut RwTransaction,
        f: &mut F,
    ) -> Result<(), FatalStorageError>
    where
        F: FnMut(&mut RwCursor<'a>, V) -> Result<(), FatalStorageError>,
    {
        let mut cursor = txn.open_rw_cursor(self.legacy)?;
        for row in cursor.iter() {
            let (_, raw_val) = row?;
            let value: V::Legacy = lmdb_ext::deserialize(raw_val)?;
            f(&mut cursor, value.into())?;
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

    /// Returns the keys from the `current` database only.
    #[cfg(test)]
    pub(super) fn keys<Tx: LmdbTransaction>(&self, txn: &Tx) -> BTreeSet<K>
    where
        K: Ord + FromBytes,
    {
        let mut cursor = txn
            .open_ro_cursor(self.current)
            .expect("should create cursor");

        cursor
            .iter()
            .map(Result::unwrap)
            .map(|(raw_key, _)| {
                bytesrepr::deserialize(raw_key.to_vec()).expect("malformed key in DB")
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
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
            let env = super::super::new_environment(1024 * 1024, data_dir.path()).unwrap();
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
        let mut txn = fixture.env.begin_ro_txn().unwrap();
        assert_eq!(
            fixture
                .dbs
                .get(&mut txn, &TransactionHash::from(*deploy_hash))
                .unwrap(),
            Some(Transaction::from(deploy.clone()))
        );

        // Should get the random transaction.
        assert_eq!(
            fixture.dbs.get(&mut txn, transaction_hash).unwrap(),
            Some(transaction.clone())
        );

        // Should return `Ok(None)` for non-existent data.
        let random_hash = Transaction::random(&mut fixture.rng).hash();
        assert!(fixture.dbs.get(&mut txn, &random_hash).unwrap().is_none());
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
        let mut txn = fixture.env.begin_ro_txn().unwrap();
        assert!(fixture
            .dbs
            .exists(&mut txn, &TransactionHash::from(*deploy_hash))
            .unwrap());

        // The random transaction should exist.
        assert!(fixture.dbs.exists(&mut txn, transaction_hash).unwrap());

        // Random data should not exist.
        let random_hash = Transaction::random(&mut fixture.rng).hash();
        assert!(!fixture.dbs.exists(&mut txn, &random_hash).unwrap());
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
            .exists(&mut txn, &TransactionHash::from(*deploy_hash))
            .unwrap());

        // Should delete the random transaction.
        fixture.dbs.delete(&mut txn, transaction_hash).unwrap();
        assert!(!fixture.dbs.exists(&mut txn, transaction_hash).unwrap());

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
        let mut visitor = |cursor: &mut RwCursor, transaction: Transaction| {
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
        let mut txn = fixture.env.begin_ro_txn().unwrap();
        for transaction_hash in fixture.random_transactions.keys() {
            assert!(!fixture.dbs.exists(&mut txn, transaction_hash).unwrap());
        }

        // Ensure a second run is a no-op.
        let mut visitor = |_cursor: &mut RwCursor, _transaction: Transaction| {
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
        let mut visitor = |cursor: &mut RwCursor, transaction: Transaction| {
            cursor.del(WriteFlags::empty()).unwrap();
            match transaction {
                Transaction::Deploy(deploy) => {
                    let _ = visited.insert(*deploy.hash(), deploy);
                }
                Transaction::V1(_) => unreachable!(),
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
        let mut txn = fixture.env.begin_ro_txn().unwrap();
        for deploy_hash in fixture.legacy_transactions.keys() {
            assert!(!fixture
                .dbs
                .exists(&mut txn, &TransactionHash::from(*deploy_hash))
                .unwrap());
        }

        // Ensure a second run is a no-op.
        let mut visitor = |_cursor: &mut RwCursor, _transaction: Transaction| {
            panic!("should never get called");
        };
        let mut txn = fixture.env.begin_rw_txn().unwrap();
        fixture
            .dbs
            .for_each_value_in_legacy(&mut txn, &mut visitor)
            .unwrap();
    }
}
