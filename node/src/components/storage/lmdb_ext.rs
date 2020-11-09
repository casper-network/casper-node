//! LMDB extensions.
//!
//! Various traits and helper functions to extend the lower levele LMDB functions.

use lmdb::{Database, Environment, RoTransaction, RwTransaction, Transaction, WriteFlags};
use serde::{de::DeserializeOwned, Serialize};

use super::serialization::deser;

/// Additional methods on `Environment`.
pub(super) trait EnvironmentExt {
    /// Creates a new read-only transaction.
    ///
    /// # Panics
    ///
    /// Panics if creating the transaction fails.
    fn ro_transaction(&self) -> RoTransaction;

    /// Creates a new read-write transaction.
    ///
    /// # Panics
    ///
    /// Panics if creating the transaction fails.
    fn rw_transaction(&self) -> RwTransaction;
}

/// Additional methods on transaction.
pub(super) trait TransactionExt {
    /// Helper function to load a value from a database.
    ///
    /// # Panics
    ///
    /// Panics if a value has been successfully loaded from the database but could not be
    /// deserialized or a database error occurred.
    fn get_value<K: AsRef<[u8]>, V: DeserializeOwned>(
        &mut self,
        db: Database,
        key: &K,
    ) -> Option<V>;
}

/// Additional methods on write transactions.
pub(super) trait WriteTransactionExt {
    /// Commits transaction results.
    ///
    /// # Panics
    ///
    /// Panics if a database error occurs.
    fn commit_ok(self);

    /// Helper function to write a value to a database.
    ///
    /// Returns `true` if the value has actually been written.
    ///
    /// # Panics
    ///
    /// Panics if a database error occurs.
    fn put_value<K: AsRef<[u8]>, V: Serialize>(&mut self, db: Database, key: &K, value: &V)
        -> bool;
}

impl EnvironmentExt for Environment {
    #[inline]
    fn ro_transaction(&self) -> RoTransaction {
        self.begin_ro_txn()
            .expect("could not start new read-only transaction")
    }
    #[inline]
    fn rw_transaction(&self) -> RwTransaction {
        self.begin_rw_txn()
            .expect("could not start new read-write transaction")
    }
}

impl<T> TransactionExt for T
where
    T: Transaction,
{
    #[inline]
    fn get_value<K: AsRef<[u8]>, V: DeserializeOwned>(
        &mut self,
        db: Database,
        key: &K,
    ) -> Option<V> {
        match self.get(db, key) {
          Ok(raw) => Some(deser(raw)),
          Err(lmdb::Error::NotFound) => None,
          Err(err) => panic!("error loading value from database. this is a bug or a sign of database corruption: {:?}", err)
      }
    }
}

impl WriteTransactionExt for RwTransaction<'_> {
    fn commit_ok(self) {
        self.commit().expect("could not commit transaction")
    }

    fn put_value<K: AsRef<[u8]>, V: Serialize>(
        &mut self,
        db: Database,
        key: &K,
        value: &V,
    ) -> bool {
        let buf = bincode::serialize(value)
            .expect("serialization of value failed. this is a serious bug");

        // TODO: To accurately report whether or not a value already exists, we need to check
        //       beforehand, as we are not using the `WriteFlags::NO_OVERWRITE` flag (see TODO
        //       below). Once fixed, this step can be remove
        //
        let exists = self.get(db, key).is_ok();

        match self.put(
            db,
            key,
            &buf,
            // TODO: this should be changed back to `WriteFlags::NO_OVERWRITE` once the mutable
            //       data (i.e. blocks' proofs) are handled via metadata as per deploys'
            //       execution results.
            WriteFlags::empty(),
            // WriteFlags::NO_OVERWRITE
        ) {
            Ok(()) => {
                //true
                !exists
            }
            Err(lmdb::Error::KeyExist) => false,
            Err(err) => panic!(
                "error storing value to database. this is a bug, or a misconfiguration: {:?}",
                err
            ),
        }
    }
}
