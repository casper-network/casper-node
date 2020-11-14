//! LMDB extensions.
//!
//! Various traits and helper functions to extend the lower levele LMDB functions. Unifies
//! lower-level storage errors from lmdb and serialization issues.

use lmdb::{Database, RwTransaction, Transaction, WriteFlags};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

use super::serialization::{deser, ser};

/// Error wrapper for lower-level storage errors.
///
/// Used to classify storage errors, allowing more accurate reporting on potential issues and
/// crashes. Indicates how to proceed (clearing storage entirely or just restarting) in most cases.
///
/// Note that accessing a storage with an incompatible version of this software is also considered a
/// case of corruption.
#[derive(Debug, Error)]
pub enum LmdbExtError {
    /// The internal database is corrupted and can probably not be salvaged.
    #[error("internal storage corrupted: {0}")]
    Corrupted(Box<dyn std::error::Error + Send + Sync>),
    /// A resource has been exhausted a runtime, restarting (potentially with different settings)
    /// might fix the problem. Storage integrity is still intact.
    #[error("storage exhausted resource (but still intact): {0}")]
    ResourceExhausted(lmdb::Error),
    /// Error neither corruption nor resource exhaustion occured, likely a programming error.
    #[error("unknown LMDB storage error, likely from a bug: {0}")]
    Other(lmdb::Error),
}

// Classifies an `lmdb::Error` according to our scheme. This one of the rare cases where we accept a
// blanked `From<>` implementation for error type conversion.
impl From<lmdb::Error> for LmdbExtError {
    fn from(lmdb_error: lmdb::Error) -> Self {
        match lmdb_error {
            lmdb::Error::PageNotFound
            | lmdb::Error::Corrupted
            | lmdb::Error::Panic
            | lmdb::Error::VersionMismatch
            | lmdb::Error::Invalid
            | lmdb::Error::Incompatible => LmdbExtError::Corrupted(Box::new(lmdb_error)),

            lmdb::Error::MapFull
            | lmdb::Error::DbsFull
            | lmdb::Error::ReadersFull
            | lmdb::Error::TlsFull
            | lmdb::Error::TxnFull
            | lmdb::Error::CursorFull
            | lmdb::Error::PageFull
            | lmdb::Error::MapResized => LmdbExtError::ResourceExhausted(lmdb_error),

            lmdb::Error::NotFound
            | lmdb::Error::BadRslot
            | lmdb::Error::BadTxn
            | lmdb::Error::BadValSize
            | lmdb::Error::BadDbi
            | lmdb::Error::KeyExist
            | lmdb::Error::Other(_) => LmdbExtError::Other(lmdb_error),
        }
    }
}

/// Additional methods on transaction.
pub(super) trait TransactionExt {
    /// Helper function to load a value from a database.
    fn get_value<K: AsRef<[u8]>, V: DeserializeOwned>(
        &mut self,
        db: Database,
        key: &K,
    ) -> Result<Option<V>, LmdbExtError>;
}

/// Additional methods on write transactions.
pub(super) trait WriteTransactionExt {
    /// Helper function to write a value to a database.
    ///
    /// Returns `true` if the value has actually been written, `false` if the key already existed.
    ///
    /// # Panics
    ///
    /// Panics if a database error occurs.
    fn put_value<K: AsRef<[u8]>, V: Serialize>(
        &mut self,
        db: Database,
        key: &K,
        value: &V,
    ) -> Result<bool, LmdbExtError>;
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
    ) -> Result<Option<V>, LmdbExtError> {
        match self.get(db, key) {
            // Deserialization failures are likely due to storage corruption.
            Ok(raw) => Some(deser(raw).map_err(LmdbExtError::Corrupted)).transpose(),
            Err(lmdb::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

impl WriteTransactionExt for RwTransaction<'_> {
    fn put_value<K: AsRef<[u8]>, V: Serialize>(
        &mut self,
        db: Database,
        key: &K,
        value: &V,
    ) -> Result<bool, LmdbExtError> {
        let buffer = ser(value).expect("TODO");

        match self.put(db, key, &buffer, WriteFlags::NO_OVERWRITE) {
            Ok(()) => Ok(true),
            // If we did not add the value due to it already existing, just return `false`.
            Err(lmdb::Error::KeyExist) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}
