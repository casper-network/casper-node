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

use std::any::TypeId;

use lmdb::{Database, RwTransaction, Transaction, WriteFlags};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tracing::warn;

use crate::block_store::types::{ApprovalsHashes, DeployMetadataV1};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    execution::ExecutionResult,
    system::auction::UnbondingPurse,
    BlockBody, BlockHeader, BlockSignatures, Deploy, DeployHash, FinalizedApprovals, Transfer,
};

const UNBONDING_PURSE_V2_MAGIC_BYTES: &[u8] = &[121, 17, 133, 179, 91, 63, 69, 222];

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
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct BytesreprError(pub bytesrepr::Error);

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
    fn get_value<K: AsRef<[u8]>, V: 'static + DeserializeOwned>(
        &self,
        db: Database,
        key: &K,
    ) -> Result<Option<V>, LmdbExtError>;

    /// Returns `true` if the given key has an entry in the given database.
    fn value_exists<K: AsRef<[u8]>>(&self, db: Database, key: &K) -> Result<bool, LmdbExtError>;

    /// Helper function to load a value from a database using the `bytesrepr` `ToBytes`/`FromBytes`
    /// serialization.
    fn get_value_bytesrepr<K: ToBytes, V: FromBytes>(
        &self,
        db: Database,
        key: &K,
    ) -> Result<Option<V>, LmdbExtError>;

    fn value_exists_bytesrepr<K: ToBytes>(
        &self,
        db: Database,
        key: &K,
    ) -> Result<bool, LmdbExtError>;
}

/// Additional methods on write transactions.
pub(super) trait WriteTransactionExt {
    /// Helper function to write a value to a database.
    ///
    /// Returns `true` if the value has actually been written, `false` if the key already existed.
    ///
    /// Setting `overwrite` to true will cause the value to always be written instead.
    fn put_value<K: AsRef<[u8]>, V: 'static + Serialize>(
        &mut self,
        db: Database,
        key: &K,
        value: &V,
        overwrite: bool,
    ) -> Result<bool, LmdbExtError>;

    /// Helper function to write a value to a database using the `bytesrepr` `ToBytes`/`FromBytes`
    /// serialization.
    ///
    /// Returns `true` if the value has actually been written, `false` if the key already existed.
    ///
    /// Setting `overwrite` to true will cause the value to always be written instead.
    fn put_value_bytesrepr<K: ToBytes, V: ToBytes>(
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
    fn get_value<K: AsRef<[u8]>, V: 'static + DeserializeOwned>(
        &self,
        db: Database,
        key: &K,
    ) -> Result<Option<V>, LmdbExtError> {
        match self.get(db, key) {
            // Deserialization failures are likely due to storage corruption.
            Ok(raw) => deserialize_internal(raw),
            Err(lmdb::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    #[inline]
    fn value_exists<K: AsRef<[u8]>>(&self, db: Database, key: &K) -> Result<bool, LmdbExtError> {
        match self.get(db, key) {
            Ok(_raw) => Ok(true),
            Err(lmdb::Error::NotFound) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    #[inline]
    fn get_value_bytesrepr<K: ToBytes, V: FromBytes>(
        &self,
        db: Database,
        key: &K,
    ) -> Result<Option<V>, LmdbExtError> {
        let serialized_key = serialize_bytesrepr(key)?;
        match self.get(db, &serialized_key) {
            // Deserialization failures are likely due to storage corruption.
            Ok(raw) => deserialize_bytesrepr(raw).map(Some),
            Err(lmdb::Error::NotFound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    #[inline]
    fn value_exists_bytesrepr<K: ToBytes>(
        &self,
        db: Database,
        key: &K,
    ) -> Result<bool, LmdbExtError> {
        let serialized_key = serialize_bytesrepr(key)?;
        match self.get(db, &serialized_key) {
            Ok(_raw) => Ok(true),
            Err(lmdb::Error::NotFound) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

/// Serializes `value` into the buffer.
/// In case the `value` is of the `UnbondingPurse` type it uses the specialized
/// function to provide compatibility with the legacy version of the `UnbondingPurse` struct.
/// See [`serialize_unbonding_purse`] for more details.
// TODO: Get rid of the 'static bound.
pub(crate) fn serialize_internal<V: 'static + Serialize>(
    value: &V,
) -> Result<Vec<u8>, LmdbExtError> {
    let buffer = if TypeId::of::<UnbondingPurse>() == TypeId::of::<V>() {
        serialize_unbonding_purse(value)?
    } else {
        serialize(value)?
    };
    Ok(buffer)
}

/// Deserializes an object from the raw bytes.
/// In case the expected object is of the `UnbondingPurse` type it uses the specialized
/// function to provide compatibility with the legacy version of the `UnbondingPurse` struct.
/// See [`deserialize_unbonding_purse`] for more details.
pub(crate) fn deserialize_internal<V: 'static + DeserializeOwned>(
    raw: &[u8],
) -> Result<Option<V>, LmdbExtError> {
    if TypeId::of::<UnbondingPurse>() == TypeId::of::<V>() {
        deserialize_unbonding_purse(raw).map(Some)
    } else {
        deserialize(raw).map(Some)
    }
}

impl WriteTransactionExt for RwTransaction<'_> {
    fn put_value<K: AsRef<[u8]>, V: 'static + Serialize>(
        &mut self,
        db: Database,
        key: &K,
        value: &V,
        overwrite: bool,
    ) -> Result<bool, LmdbExtError> {
        let buffer = serialize_internal(value)?;

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

    fn put_value_bytesrepr<K: ToBytes, V: ToBytes>(
        &mut self,
        db: Database,
        key: &K,
        value: &V,
        overwrite: bool,
    ) -> Result<bool, LmdbExtError> {
        let serialized_key = serialize_bytesrepr(key)?;
        let serialized_value = serialize_bytesrepr(value)?;

        let flags = if overwrite {
            WriteFlags::empty()
        } else {
            WriteFlags::NO_OVERWRITE
        };

        match self.put(db, &serialized_key, &serialized_value, flags) {
            Ok(()) => Ok(true),
            // If we did not add the value due to it already existing, just return `false`.
            Err(lmdb::Error::KeyExist) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

/// Deserializes from a buffer.
#[inline(always)]
pub(super) fn deserialize<T: DeserializeOwned + 'static>(raw: &[u8]) -> Result<T, LmdbExtError> {
    match bincode::deserialize(raw) {
        Ok(value) => Ok(value),
        Err(err) => {
            // unfortunately, type_name is unstable
            let type_name = {
                if TypeId::of::<DeployMetadataV1>() == TypeId::of::<T>() {
                    "DeployMetadataV1".to_string()
                } else if TypeId::of::<BlockHeader>() == TypeId::of::<T>() {
                    "BlockHeader".to_string()
                } else if TypeId::of::<BlockBody>() == TypeId::of::<T>() {
                    "BlockBody".to_string()
                } else if TypeId::of::<BlockSignatures>() == TypeId::of::<T>() {
                    "BlockSignatures".to_string()
                } else if TypeId::of::<DeployHash>() == TypeId::of::<T>() {
                    "DeployHash".to_string()
                } else if TypeId::of::<Deploy>() == TypeId::of::<T>() {
                    "Deploy".to_string()
                } else if TypeId::of::<ApprovalsHashes>() == TypeId::of::<T>() {
                    "ApprovalsHashes".to_string()
                } else if TypeId::of::<FinalizedApprovals>() == TypeId::of::<T>() {
                    "FinalizedApprovals".to_string()
                } else if TypeId::of::<ExecutionResult>() == TypeId::of::<T>() {
                    "ExecutionResult".to_string()
                } else if TypeId::of::<Vec<Transfer>>() == TypeId::of::<T>() {
                    "Transfers".to_string()
                } else {
                    format!("{:?}", TypeId::of::<T>())
                }
            };
            warn!(?err, ?raw, "{}: bincode deserialization failed", type_name);
            Err(LmdbExtError::DataCorrupted(Box::new(err)))
        }
    }
}

/// Returns `true` if the specified bytes represent the legacy version of `UnbondingPurse`.
fn is_legacy(raw: &[u8]) -> bool {
    !raw.starts_with(UNBONDING_PURSE_V2_MAGIC_BYTES)
}

/// Deserializes `UnbondingPurse` from a buffer.
/// To provide backward compatibility with the previous version of the `UnbondingPurse`,
/// it checks if the raw bytes stream begins with "magic bytes". If yes, the magic bytes are
/// stripped and the struct is deserialized as a new version. Otherwise, the raw bytes
/// are treated as bytes representing the legacy `UnbondingPurse` and deserialized accordingly.
/// In order for the latter scenario to work, the raw bytes stream is extended with
/// bytes that represent the `None` serialized with `bincode` - these bytes simulate
/// the existence of the `new_validator` field added to the `UnbondingPurse` struct.
pub(super) fn deserialize_unbonding_purse<T: DeserializeOwned + 'static>(
    raw: &[u8],
) -> Result<T, LmdbExtError> {
    const BINCODE_ENCODED_NONE: [u8; 4] = [0; 4];
    if is_legacy(raw) {
        deserialize(&[raw, &BINCODE_ENCODED_NONE].concat())
    } else {
        deserialize(&raw[UNBONDING_PURSE_V2_MAGIC_BYTES.len()..])
    }
}

/// Serializes into a buffer.
#[inline(always)]
pub(super) fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, LmdbExtError> {
    bincode::serialize(value).map_err(|err| LmdbExtError::Other(Box::new(err)))
}

/// Serializes `UnbondingPurse` into a buffer.
/// To provide backward compatibility with the previous version of the `UnbondingPurse`,
/// the serialized bytes are prefixed with the "magic bytes", which will be used by the
/// deserialization routine to detect the version of the `UnbondingPurse` struct.
#[inline(always)]
pub(super) fn serialize_unbonding_purse<T: Serialize>(value: &T) -> Result<Vec<u8>, LmdbExtError> {
    let mut serialized = UNBONDING_PURSE_V2_MAGIC_BYTES.to_vec();
    serialized.extend(bincode::serialize(value).map_err(|err| LmdbExtError::Other(Box::new(err)))?);
    Ok(serialized)
}

/// Deserializes from a buffer.
#[inline(always)]
pub(super) fn deserialize_bytesrepr<T: FromBytes>(raw: &[u8]) -> Result<T, LmdbExtError> {
    T::from_bytes(raw)
        .map(|val| val.0)
        .map_err(|err| LmdbExtError::DataCorrupted(Box::new(BytesreprError(err))))
}

/// Serializes into a buffer.
#[inline(always)]
pub(super) fn serialize_bytesrepr<T: ToBytes>(value: &T) -> Result<Vec<u8>, LmdbExtError> {
    value
        .to_bytes()
        .map_err(|err| LmdbExtError::Other(Box::new(BytesreprError(err))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::{AccessRights, EraId, PublicKey, SecretKey, URef, U512};

    #[test]
    fn should_read_legacy_unbonding_purse() {
        // These bytes represent the `UnbondingPurse` struct with the `new_validator` field removed
        // and serialized with `bincode`.
        // In theory, we can generate these bytes by serializing the `WithdrawPurse`, but at some
        // point, these two structs may diverge and it's a safe bet to rely on the bytes
        // that are consistent with what we keep in the current storage.
        const LEGACY_BYTES: &str = "0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e07010000002000000000000000197f6b23e16c8532c6abc838facd5ea789be0c76b2920334039bfa8b3d368d610100000020000000000000004508a07aa941707f3eb2db94c8897a80b2c1197476b6de213ac273df7d86c4ffffffffffffffffff40feffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

        let decoded = base16::decode(LEGACY_BYTES).expect("decode");
        let deserialized: UnbondingPurse = deserialize_internal(&decoded)
            .expect("should deserialize w/o error")
            .expect("should be Some");

        // Make sure the new field is set to default.
        assert_eq!(*deserialized.new_validator(), Option::default())
    }

    #[test]
    fn unbonding_purse_serialization_roundtrip() {
        let original = UnbondingPurse::new(
            URef::new([14; 32], AccessRights::READ_ADD_WRITE),
            {
                let secret_key =
                    SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
                PublicKey::from(&secret_key)
            },
            {
                let secret_key =
                    SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap();
                PublicKey::from(&secret_key)
            },
            EraId::MAX,
            U512::max_value() - 1,
            Some({
                let secret_key =
                    SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap();
                PublicKey::from(&secret_key)
            }),
        );

        let serialized = serialize_internal(&original).expect("serialization");
        let deserialized: UnbondingPurse = deserialize_internal(&serialized)
            .expect("should deserialize w/o error")
            .expect("should be Some");

        assert_eq!(original, deserialized);

        // Explicitly assert that the `new_validator` is not `None`
        assert!(deserialized.new_validator().is_some())
    }
}
