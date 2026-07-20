//! EVM system contract support.

use casper_types::{
    evm, ByteCode, ByteCodeKind, CLValue, CLValueError, EvmAddr, EvmConfig, EvmSpec, Key,
    StoredValue,
};
use thiserror::Error;

use crate::{
    eip4788,
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyError},
};

/// Error returned while installing or validating an EVM predeploy.
#[derive(Debug, Error)]
pub(crate) enum EvmPredeployError {
    /// Failed to read from the tracking copy.
    #[error(transparent)]
    TrackingCopy(#[from] TrackingCopyError),
    /// Failed to build or decode a CLValue.
    #[error("CLValue error: {0}")]
    CLValue(String),
    /// The reserved predeploy address already has non-empty code.
    #[error("reserved EVM predeploy address has conflicting code hash {actual}")]
    ConflictingCodeHash {
        /// Actual code hash found under the reserved address.
        actual: evm::Hash,
    },
    /// The expected bytecode key contains incompatible data.
    #[error("EVM predeploy bytecode conflict at {key}: {details}")]
    ConflictingByteCode {
        /// Global-state key where the conflict was found.
        key: Box<Key>,
        /// Conflict details.
        details: String,
    },
    /// An EVM predeploy key contains an unexpected stored value.
    #[error("unexpected stored value at {key}: expected {expected}, found {found}")]
    UnexpectedStoredValue {
        /// Global-state key that was read.
        key: Box<Key>,
        /// Expected stored-value variant.
        expected: &'static str,
        /// Actual stored-value variant.
        found: String,
    },
}

impl From<CLValueError> for EvmPredeployError {
    fn from(error: CLValueError) -> Self {
        EvmPredeployError::CLValue(error.to_string())
    }
}

/// Returns whether EIP-4788 should be installed for the supplied EVM config.
pub(crate) fn should_upsert_eip4788_predeploy(config: &EvmConfig) -> bool {
    config.enabled && config.spec >= EvmSpec::Prague
}

/// Idempotently installs the EIP-4788 beacon roots predeploy.
pub(crate) fn upsert_eip4788_predeploy<R>(
    tracking_copy: &mut TrackingCopy<R>,
) -> Result<(), EvmPredeployError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    upsert_beacon_roots_code_hash(tracking_copy)?;
    upsert_beacon_roots_byte_code(tracking_copy)?;
    Ok(())
}

fn beacon_roots_code_hash_key() -> Key {
    Key::Evm(EvmAddr::CodeHash(eip4788::BEACON_ROOTS_ADDRESS))
}

fn beacon_roots_byte_code_key() -> Key {
    Key::Evm(EvmAddr::ByteCode(eip4788::beacon_roots_code_hash()))
}

fn beacon_roots_code_hash_value() -> Result<StoredValue, EvmPredeployError> {
    Ok(StoredValue::CLValue(CLValue::from_t(
        eip4788::beacon_roots_code_hash(),
    )?))
}

fn beacon_roots_byte_code_value() -> StoredValue {
    StoredValue::ByteCode(ByteCode::new(
        ByteCodeKind::EvmPrague,
        eip4788::BEACON_ROOTS_CODE.to_vec(),
    ))
}

#[cfg(test)]
fn beacon_roots_predeploy_entries() -> Result<Vec<(Key, StoredValue)>, EvmPredeployError> {
    Ok(vec![
        (
            beacon_roots_code_hash_key(),
            beacon_roots_code_hash_value()?,
        ),
        (beacon_roots_byte_code_key(), beacon_roots_byte_code_value()),
    ])
}

fn upsert_beacon_roots_code_hash<R>(
    tracking_copy: &mut TrackingCopy<R>,
) -> Result<(), EvmPredeployError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let key = beacon_roots_code_hash_key();
    let expected = eip4788::beacon_roots_code_hash();
    match tracking_copy.read(&key)? {
        None => {
            tracking_copy.write(key, beacon_roots_code_hash_value()?);
        }
        Some(StoredValue::CLValue(cl_value)) => {
            let actual = cl_value.to_t::<evm::Hash>()?;
            if actual == expected {
                return Ok(());
            }
            if actual == evm::EMPTY_CODE_HASH || actual.is_zero() {
                tracking_copy.write(key, beacon_roots_code_hash_value()?);
                return Ok(());
            }
            return Err(EvmPredeployError::ConflictingCodeHash { actual });
        }
        Some(stored_value) => {
            return Err(EvmPredeployError::UnexpectedStoredValue {
                key: Box::new(key),
                expected: "StoredValue::CLValue(evm::Hash)",
                found: stored_value.type_name(),
            });
        }
    }
    Ok(())
}

fn upsert_beacon_roots_byte_code<R>(
    tracking_copy: &mut TrackingCopy<R>,
) -> Result<(), EvmPredeployError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let key = beacon_roots_byte_code_key();
    match tracking_copy.read(&key)? {
        None => {
            tracking_copy.write(key, beacon_roots_byte_code_value());
        }
        Some(StoredValue::ByteCode(byte_code)) => {
            if byte_code.kind() == ByteCodeKind::EvmPrague
                && byte_code.bytes() == eip4788::BEACON_ROOTS_CODE
            {
                return Ok(());
            }
            return Err(EvmPredeployError::ConflictingByteCode {
                key: Box::new(key),
                details: format!(
                    "expected Prague EIP-4788 bytecode, found kind {} with {} bytes",
                    byte_code.kind(),
                    byte_code.bytes().len()
                ),
            });
        }
        Some(stored_value) => {
            return Err(EvmPredeployError::UnexpectedStoredValue {
                key: Box::new(key),
                expected: "StoredValue::ByteCode(EvmPrague)",
                found: stored_value.type_name(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use casper_types::{ByteCode, ByteCodeKind, CLValue, EvmAddr};

    use super::*;
    use crate::global_state::state::{self, lmdb::LmdbGlobalStateView, StateProvider as _};

    fn tracking_copy(
        initial_data: impl IntoIterator<Item = (Key, StoredValue)>,
    ) -> (TrackingCopy<LmdbGlobalStateView>, impl Send) {
        let (global_state, root_hash, tempdir) =
            state::lmdb::make_temporary_global_state(initial_data);
        let reader = global_state
            .checkout(root_hash)
            .expect("checkout should not fail")
            .expect("root should exist");
        (TrackingCopy::new(reader, 5, false), tempdir)
    }

    fn read(tracking_copy: &mut TrackingCopy<LmdbGlobalStateView>, key: &Key) -> StoredValue {
        tracking_copy
            .read(key)
            .expect("read should not fail")
            .expect("value should exist")
    }

    #[test]
    fn should_upsert_for_enabled_prague_or_later_evm() {
        assert!(!should_upsert_eip4788_predeploy(&EvmConfig::default()));

        let config = EvmConfig {
            enabled: true,
            ..Default::default()
        };
        assert!(should_upsert_eip4788_predeploy(&config));
    }

    #[test]
    fn upsert_creates_missing_predeploy() {
        let (mut tracking_copy, _tempdir) = tracking_copy([]);

        upsert_eip4788_predeploy(&mut tracking_copy).expect("upsert should succeed");

        assert_eq!(
            read(&mut tracking_copy, &beacon_roots_code_hash_key()),
            beacon_roots_code_hash_value().expect("code hash value should build")
        );
        assert_eq!(
            read(&mut tracking_copy, &beacon_roots_byte_code_key()),
            beacon_roots_byte_code_value()
        );
    }

    #[test]
    fn upsert_repairs_missing_bytecode() {
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            beacon_roots_code_hash_key(),
            beacon_roots_code_hash_value().expect("code hash value should build"),
        )]);

        upsert_eip4788_predeploy(&mut tracking_copy).expect("upsert should succeed");

        assert_eq!(
            read(&mut tracking_copy, &beacon_roots_byte_code_key()),
            beacon_roots_byte_code_value()
        );
    }

    #[test]
    fn upsert_noops_when_predeploy_is_present() {
        let (mut tracking_copy, _tempdir) =
            tracking_copy(beacon_roots_predeploy_entries().expect("entries should build"));

        upsert_eip4788_predeploy(&mut tracking_copy).expect("upsert should succeed");
    }

    #[test]
    fn upsert_rejects_conflicting_non_empty_code_hash() {
        let conflicting_hash = evm::Hash::new([1; evm::HASH_LENGTH]);
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            beacon_roots_code_hash_key(),
            StoredValue::CLValue(CLValue::from_t(conflicting_hash).expect("hash should encode")),
        )]);

        let error = upsert_eip4788_predeploy(&mut tracking_copy)
            .expect_err("conflicting code hash should fail");

        assert!(matches!(
            error,
            EvmPredeployError::ConflictingCodeHash {
                actual
            } if actual == conflicting_hash
        ));
    }

    #[test]
    fn upsert_rejects_conflicting_bytecode() {
        let (mut tracking_copy, _tempdir) = tracking_copy([
            (
                beacon_roots_code_hash_key(),
                beacon_roots_code_hash_value().expect("code hash value should build"),
            ),
            (
                beacon_roots_byte_code_key(),
                StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmPrague, vec![0xfe])),
            ),
        ]);

        let error = upsert_eip4788_predeploy(&mut tracking_copy)
            .expect_err("conflicting bytecode should fail");

        assert!(matches!(
            error,
            EvmPredeployError::ConflictingByteCode { .. }
        ));
    }

    #[test]
    fn upsert_repairs_empty_code_hash() {
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            beacon_roots_code_hash_key(),
            StoredValue::CLValue(
                CLValue::from_t(evm::EMPTY_CODE_HASH).expect("hash should encode"),
            ),
        )]);

        upsert_eip4788_predeploy(&mut tracking_copy).expect("upsert should succeed");

        assert_eq!(
            read(&mut tracking_copy, &beacon_roots_code_hash_key()),
            beacon_roots_code_hash_value().expect("code hash value should build")
        );
    }

    #[test]
    fn upsert_rejects_unexpected_code_hash_value() {
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            beacon_roots_code_hash_key(),
            StoredValue::CLValue(
                CLValue::from_t(Key::Evm(EvmAddr::Account(eip4788::BEACON_ROOTS_ADDRESS)))
                    .expect("key should encode"),
            ),
        )]);

        let error = upsert_eip4788_predeploy(&mut tracking_copy)
            .expect_err("invalid code hash value should fail");

        assert!(matches!(error, EvmPredeployError::CLValue(_)));
    }
}
