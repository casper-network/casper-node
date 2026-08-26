//! EVM system contract support.

use casper_types::{
    evm, ByteCode, ByteCodeKind, CLValue, CLValueError, EvmAddr, EvmConfig, EvmSpec, Key,
    StoredValue,
};
use thiserror::Error;

use crate::{
    eip2935, eip4788,
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

#[derive(Clone, Copy)]
struct EvmPredeploy {
    name: &'static str,
    address: evm::Address,
    code: &'static [u8],
    code_hash: evm::Hash,
}

impl EvmPredeploy {
    fn eip2935() -> Self {
        Self {
            name: "EIP-2935",
            address: eip2935::BLOCK_HASH_HISTORY_ADDRESS,
            code: eip2935::BLOCK_HASH_HISTORY_CODE,
            code_hash: eip2935::block_hash_history_code_hash(),
        }
    }

    fn eip4788() -> Self {
        Self {
            name: "EIP-4788",
            address: eip4788::BEACON_ROOTS_ADDRESS,
            code: eip4788::BEACON_ROOTS_CODE,
            code_hash: eip4788::beacon_roots_code_hash(),
        }
    }

    fn code_hash_key(self) -> Key {
        Key::Evm(EvmAddr::CodeHash(self.address))
    }

    fn byte_code_key(self) -> Key {
        Key::Evm(EvmAddr::ByteCode(self.code_hash))
    }

    fn code_hash_value(self) -> Result<StoredValue, EvmPredeployError> {
        Ok(StoredValue::CLValue(CLValue::from_t(self.code_hash)?))
    }

    fn byte_code_value(self) -> StoredValue {
        StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmPrague, self.code.to_vec()))
    }
}

fn prague_predeploys() -> [EvmPredeploy; 2] {
    [EvmPredeploy::eip4788(), EvmPredeploy::eip2935()]
}

/// Returns whether Prague EVM predeploys should be installed for the supplied EVM config.
pub(crate) fn should_upsert_prague_predeploys(config: &EvmConfig) -> bool {
    config.enabled && config.spec >= EvmSpec::Prague
}

/// Idempotently installs the Prague EVM predeploys.
pub(crate) fn upsert_prague_predeploys<R>(
    tracking_copy: &mut TrackingCopy<R>,
) -> Result<(), EvmPredeployError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    for predeploy in prague_predeploys() {
        upsert_code_hash(tracking_copy, predeploy)?;
        upsert_byte_code(tracking_copy, predeploy)?;
    }
    Ok(())
}

#[cfg(test)]
fn predeploy_entries(
    predeploy: EvmPredeploy,
) -> Result<Vec<(Key, StoredValue)>, EvmPredeployError> {
    Ok(vec![
        (predeploy.code_hash_key(), predeploy.code_hash_value()?),
        (predeploy.byte_code_key(), predeploy.byte_code_value()),
    ])
}

#[cfg(test)]
fn prague_predeploy_entries() -> Result<Vec<(Key, StoredValue)>, EvmPredeployError> {
    prague_predeploys()
        .iter()
        .copied()
        .map(predeploy_entries)
        .collect::<Result<Vec<_>, _>>()
        .map(|entries| entries.into_iter().flatten().collect())
}

fn upsert_code_hash<R>(
    tracking_copy: &mut TrackingCopy<R>,
    predeploy: EvmPredeploy,
) -> Result<(), EvmPredeployError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let key = predeploy.code_hash_key();
    let expected = predeploy.code_hash;
    match tracking_copy.read(&key)? {
        None => {
            tracking_copy.write(key, predeploy.code_hash_value()?);
        }
        Some(StoredValue::CLValue(cl_value)) => {
            let actual = cl_value.to_t::<evm::Hash>()?;
            if actual == expected {
                return Ok(());
            }
            if actual == evm::EMPTY_CODE_HASH || actual.is_zero() {
                tracking_copy.write(key, predeploy.code_hash_value()?);
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

fn upsert_byte_code<R>(
    tracking_copy: &mut TrackingCopy<R>,
    predeploy: EvmPredeploy,
) -> Result<(), EvmPredeployError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let key = predeploy.byte_code_key();
    match tracking_copy.read(&key)? {
        None => {
            tracking_copy.write(key, predeploy.byte_code_value());
        }
        Some(StoredValue::ByteCode(byte_code)) => {
            if byte_code.kind() == ByteCodeKind::EvmPrague && byte_code.bytes() == predeploy.code {
                return Ok(());
            }
            return Err(EvmPredeployError::ConflictingByteCode {
                key: Box::new(key),
                details: format!(
                    "expected Prague {} bytecode, found kind {} with {} bytes",
                    predeploy.name,
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
        (TrackingCopy::new(reader, 5), tempdir)
    }

    fn read(tracking_copy: &mut TrackingCopy<LmdbGlobalStateView>, key: &Key) -> StoredValue {
        tracking_copy
            .read(key)
            .expect("read should not fail")
            .expect("value should exist")
    }

    fn assert_predeploy_present(
        tracking_copy: &mut TrackingCopy<LmdbGlobalStateView>,
        predeploy: EvmPredeploy,
    ) {
        assert_eq!(
            read(tracking_copy, &predeploy.code_hash_key()),
            predeploy
                .code_hash_value()
                .expect("code hash value should build")
        );
        assert_eq!(
            read(tracking_copy, &predeploy.byte_code_key()),
            predeploy.byte_code_value()
        );
    }

    #[test]
    fn should_upsert_prague_predeploys_for_enabled_prague_or_later_evm() {
        assert!(!should_upsert_prague_predeploys(&EvmConfig::default()));

        let config = EvmConfig {
            enabled: true,
            ..Default::default()
        };
        assert!(should_upsert_prague_predeploys(&config));
    }

    #[test]
    fn upsert_creates_missing_prague_predeploys() {
        let (mut tracking_copy, _tempdir) = tracking_copy([]);

        upsert_prague_predeploys(&mut tracking_copy).expect("upsert should succeed");

        assert_predeploy_present(&mut tracking_copy, EvmPredeploy::eip4788());
        assert_predeploy_present(&mut tracking_copy, EvmPredeploy::eip2935());
    }

    #[test]
    fn upsert_repairs_missing_eip2935_bytecode() {
        let predeploy = EvmPredeploy::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            predeploy
                .code_hash_value()
                .expect("code hash value should build"),
        )]);

        upsert_prague_predeploys(&mut tracking_copy).expect("upsert should succeed");

        assert_predeploy_present(&mut tracking_copy, predeploy);
    }

    #[test]
    fn upsert_noops_when_prague_predeploys_are_present() {
        let (mut tracking_copy, _tempdir) =
            tracking_copy(prague_predeploy_entries().expect("entries should build"));

        upsert_prague_predeploys(&mut tracking_copy).expect("upsert should succeed");
    }

    #[test]
    fn upsert_rejects_conflicting_eip2935_non_empty_code_hash() {
        let predeploy = EvmPredeploy::eip2935();
        let conflicting_hash = evm::Hash::new([1; evm::HASH_LENGTH]);
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            StoredValue::CLValue(CLValue::from_t(conflicting_hash).expect("hash should encode")),
        )]);

        let error = upsert_prague_predeploys(&mut tracking_copy)
            .expect_err("conflicting code hash should fail");

        assert!(matches!(
            error,
            EvmPredeployError::ConflictingCodeHash {
                actual
            } if actual == conflicting_hash
        ));
    }

    #[test]
    fn upsert_rejects_conflicting_eip2935_bytecode() {
        let predeploy = EvmPredeploy::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([
            (
                predeploy.code_hash_key(),
                predeploy
                    .code_hash_value()
                    .expect("code hash value should build"),
            ),
            (
                predeploy.byte_code_key(),
                StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmPrague, vec![0xfe])),
            ),
        ]);

        let error = upsert_prague_predeploys(&mut tracking_copy)
            .expect_err("conflicting bytecode should fail");

        assert!(matches!(
            error,
            EvmPredeployError::ConflictingByteCode { .. }
        ));
    }

    #[test]
    fn upsert_repairs_empty_eip2935_code_hash() {
        let predeploy = EvmPredeploy::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            StoredValue::CLValue(
                CLValue::from_t(evm::EMPTY_CODE_HASH).expect("hash should encode"),
            ),
        )]);

        upsert_prague_predeploys(&mut tracking_copy).expect("upsert should succeed");

        assert_predeploy_present(&mut tracking_copy, predeploy);
    }

    #[test]
    fn upsert_rejects_unexpected_eip2935_code_hash_value() {
        let predeploy = EvmPredeploy::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            StoredValue::CLValue(
                CLValue::from_t(Key::Evm(EvmAddr::Account(
                    eip2935::BLOCK_HASH_HISTORY_ADDRESS,
                )))
                .expect("key should encode"),
            ),
        )]);

        let error = upsert_prague_predeploys(&mut tracking_copy)
            .expect_err("invalid code hash value should fail");

        assert!(matches!(error, EvmPredeployError::CLValue(_)));
    }
}
