//! LMDB extensions.
//!
//! Various traits and helper functions to extend the lower level LMDB functions. Unifies
//! lower-level storage errors from lmdb and serialization issues.
//!
//! ## Serialization
//!
//! The module also centralizes settings and methods for serialization for all parts of storage.
//!
//! Serialization errors are unified into a generic, type erased `std` error to allow for easy
//! interchange of the serialization format if desired.

use crate::{crypto::hash::Digest, types::BlockHash};
use lmdb::{Database, RwTransaction, Transaction, WriteFlags};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

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
    LmdbCorrupted(lmdb::Error),
    /// The data stored inside the internal database is corrupted or formatted wrong.
    #[error("internal data corrupted: {0}")]
    DataCorrupted(Box<dyn std::error::Error + Send + Sync>),
    /// A resource has been exhausted at runtime, restarting (potentially with different settings)
    /// might fix the problem. Storage integrity is still intact.
    #[error("storage exhausted resource (but still intact): {0}")]
    ResourceExhausted(lmdb::Error),
    /// Error neither corruption nor resource exhaustion occurred, likely a programming error.
    #[error("unknown LMDB or serialization error, likely from a bug: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
    /// The internal database is corrupted and can probably not be salvaged.
    #[error(
        "Block header not stored under its hash. \
         Queried block hash: {queried_block_hash}, \
         Found block header hash: {found_block_header_hash}"
    )]
    BlockHeaderNotStoredUnderItsHash {
        queried_block_hash: BlockHash,
        found_block_header_hash: BlockHash,
    },
    #[error(
        "Block body not stored under the hash in its header. \
         Queried block body hash: {queried_block_body_hash}, \
         Found block body hash: {found_block_body_hash}"
    )]
    BlockBodyNotStoredUnderItsHash {
        queried_block_body_hash: Digest,
        found_block_body_hash: Digest,
    },
    #[error(
        "Block body part is missing from storage. \
        Queried block body hash: {block_body_hash}, \
        Missing part hash: {part_hash}"
    )]
    BlockBodyPartMissing {
        block_body_hash: Digest,
        part_hash: Digest,
    },
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
            | lmdb::Error::Incompatible => LmdbExtError::LmdbCorrupted(lmdb_error),

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
            | lmdb::Error::Other(_) => LmdbExtError::Other(Box::new(lmdb_error)),
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
    /// Setting `overwrite` to true will cause the value to always be written instead.
    fn put_value<K: AsRef<[u8]>, V: Serialize>(
        &mut self,
        db: Database,
        key: &K,
        value: &V,
        overwrite: bool,
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
            Ok(raw) => deserialize(raw).map(Some),
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
        overwrite: bool,
    ) -> Result<bool, LmdbExtError> {
        let buffer = serialize(value)?;

        let flags = if overwrite {
            WriteFlags::empty()
        } else {
            WriteFlags::NO_OVERWRITE
        };

        match self.put(db, key, &buffer, flags) {
            Ok(()) => Ok(true),
            // If we did not add the value due to it already existing, just return `false`.
            Err(lmdb::Error::KeyExist) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

/// Deserializes from a buffer.
#[inline(always)]
pub(super) fn deserialize<T: DeserializeOwned>(raw: &[u8]) -> Result<T, LmdbExtError> {
    bincode::deserialize(raw).map_err(|err| LmdbExtError::DataCorrupted(Box::new(err)))
}

/// Serializes into a buffer.
#[inline(always)]
pub(super) fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, LmdbExtError> {
    bincode::serialize(value).map_err(|err| LmdbExtError::Other(Box::new(err)))
}
