//! EVM system contract support.

use std::collections::BTreeMap;

use alloy_primitives::keccak256;
use casper_types::{
    bytesrepr::Bytes, evm, ByteCode, ByteCodeKind, CLValue, CLValueError, EvmAddr, EvmConfig,
    EvmSpec, Key, StoredValue,
};
use thiserror::Error;

use crate::{
    eip2935, eip4788,
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyError},
};

/// Error returned while installing or validating an EVM contract's code.
#[derive(Debug, Error)]
pub(crate) enum EvmContractError {
    /// A configured preinstall has no runtime bytecode.
    #[error("EVM preinstall at {address} has empty runtime bytecode")]
    EmptyPreinstall {
        /// Configured address.
        address: evm::Address,
    },
    /// Failed to read from the tracking copy.
    #[error(transparent)]
    TrackingCopy(#[from] TrackingCopyError),
    /// Failed to build or decode a CLValue.
    #[error("CLValue error: {0}")]
    CLValue(String),
    /// The reserved contract address already has different non-empty code.
    #[error(
        "reserved {name} address {address} has conflicting code hash {actual}, expected {expected}"
    )]
    ConflictingCodeHash {
        /// Contract being installed.
        name: &'static str,
        /// Reserved address.
        address: evm::Address,
        /// Expected code hash.
        expected: evm::Hash,
        /// Actual code hash found under the reserved address.
        actual: evm::Hash,
    },
    /// The expected bytecode key contains incompatible data.
    #[error("EVM contract bytecode conflict at {key}: {details}")]
    ConflictingByteCode {
        /// Global-state key where the conflict was found.
        key: Box<Key>,
        /// Conflict details.
        details: String,
    },
    /// An EVM code key contains an unexpected stored value.
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

impl From<CLValueError> for EvmContractError {
    fn from(error: CLValueError) -> Self {
        EvmContractError::CLValue(error.to_string())
    }
}

#[derive(Clone, Copy)]
struct EvmContract<'a> {
    name: &'static str,
    address: evm::Address,
    code: &'a [u8],
    code_hash: evm::Hash,
}

impl<'a> EvmContract<'a> {
    fn preinstall(address: evm::Address, code: &'a [u8]) -> Self {
        Self {
            name: "EVM preinstall",
            address,
            code,
            code_hash: evm::Hash::new(keccak256(code).0),
        }
    }

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

    fn code_hash_value(self) -> Result<StoredValue, EvmContractError> {
        Ok(StoredValue::CLValue(CLValue::from_t(self.code_hash)?))
    }

    fn byte_code_value(self) -> StoredValue {
        StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmOsaka, self.code.to_vec()))
    }
}

fn osaka_predeploys() -> [EvmContract<'static>; 2] {
    [EvmContract::eip4788(), EvmContract::eip2935()]
}

/// Returns whether Osaka EVM predeploys should be installed for the supplied EVM config.
pub(crate) fn should_upsert_osaka_predeploys(config: &EvmConfig) -> bool {
    config.enabled && config.spec >= EvmSpec::Osaka
}

/// Idempotently installs the Osaka EVM predeploys.
pub(crate) fn upsert_osaka_predeploys<R>(
    tracking_copy: &mut TrackingCopy<R>,
) -> Result<(), EvmContractError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    for predeploy in osaka_predeploys() {
        upsert_contract(tracking_copy, predeploy)?;
    }
    Ok(())
}

/// Idempotently installs EVM runtime bytecode supplied by the chainspec.
pub(crate) fn upsert_preinstalls<R>(
    tracking_copy: &mut TrackingCopy<R>,
    preinstalls: &BTreeMap<evm::Address, Bytes>,
) -> Result<(), EvmContractError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    for (address, code) in preinstalls {
        if code.is_empty() {
            return Err(EvmContractError::EmptyPreinstall { address: *address });
        }
        upsert_contract(tracking_copy, EvmContract::preinstall(*address, code))?;
    }
    Ok(())
}

fn upsert_contract<R>(
    tracking_copy: &mut TrackingCopy<R>,
    contract: EvmContract<'_>,
) -> Result<(), EvmContractError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    upsert_code_hash(tracking_copy, contract)?;
    upsert_byte_code(tracking_copy, contract)
}

#[cfg(test)]
fn predeploy_entries(
    predeploy: EvmContract<'_>,
) -> Result<Vec<(Key, StoredValue)>, EvmContractError> {
    Ok(vec![
        (predeploy.code_hash_key(), predeploy.code_hash_value()?),
        (predeploy.byte_code_key(), predeploy.byte_code_value()),
    ])
}

#[cfg(test)]
fn osaka_predeploy_entries() -> Result<Vec<(Key, StoredValue)>, EvmContractError> {
    osaka_predeploys()
        .iter()
        .copied()
        .map(predeploy_entries)
        .collect::<Result<Vec<_>, _>>()
        .map(|entries| entries.into_iter().flatten().collect())
}

fn upsert_code_hash<R>(
    tracking_copy: &mut TrackingCopy<R>,
    predeploy: EvmContract<'_>,
) -> Result<(), EvmContractError>
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
            return Err(EvmContractError::ConflictingCodeHash {
                name: predeploy.name,
                address: predeploy.address,
                expected,
                actual,
            });
        }
        Some(stored_value) => {
            return Err(EvmContractError::UnexpectedStoredValue {
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
    predeploy: EvmContract<'_>,
) -> Result<(), EvmContractError>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let key = predeploy.byte_code_key();
    match tracking_copy.read(&key)? {
        None => {
            tracking_copy.write(key, predeploy.byte_code_value());
        }
        Some(StoredValue::ByteCode(byte_code)) => {
            if byte_code.kind() == ByteCodeKind::EvmOsaka && byte_code.bytes() == predeploy.code {
                return Ok(());
            }
            return Err(EvmContractError::ConflictingByteCode {
                key: Box::new(key),
                details: format!(
                    "expected Osaka {} bytecode, found kind {} with {} bytes",
                    predeploy.name,
                    byte_code.kind(),
                    byte_code.bytes().len()
                ),
            });
        }
        Some(stored_value) => {
            return Err(EvmContractError::UnexpectedStoredValue {
                key: Box::new(key),
                expected: "StoredValue::ByteCode(EvmOsaka)",
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

    fn test_contracts() -> [EvmContract<'static>; 2] {
        // Two addresses share a tiny, project-authored runtime returning 42.
        let code = &[0x60, 0x2a, 0x60, 0, 0x52, 0x60, 0x20, 0x60, 0, 0xf3];
        [
            EvmContract::preinstall(evm::Address::new([1; 20]), code),
            EvmContract::preinstall(evm::Address::new([2; 20]), code),
        ]
    }

    fn test_preinstalls() -> BTreeMap<evm::Address, Bytes> {
        test_contracts()
            .iter()
            .map(|contract| (contract.address, Bytes::from(contract.code)))
            .collect()
    }

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

    fn assert_predeploy_present(
        tracking_copy: &mut TrackingCopy<LmdbGlobalStateView>,
        predeploy: EvmContract<'_>,
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
    fn should_upsert_osaka_predeploys_for_enabled_osaka_or_later_evm() {
        assert!(!should_upsert_osaka_predeploys(&EvmConfig::default()));
        assert!(EvmConfig::default().active_system_contracts().is_empty());

        let config = EvmConfig {
            enabled: true,
            ..Default::default()
        };
        assert!(should_upsert_osaka_predeploys(&config));
        let advertised = config.active_system_contracts();
        assert_eq!(advertised.len(), osaka_predeploys().len());
        assert_eq!(
            advertised["BEACON_ROOTS_ADDRESS"],
            EvmContract::eip4788().address
        );
        assert_eq!(
            advertised["HISTORY_STORAGE_ADDRESS"],
            EvmContract::eip2935().address
        );
    }

    #[test]
    fn upsert_creates_missing_osaka_predeploys() {
        let (mut tracking_copy, _tempdir) = tracking_copy([]);

        upsert_osaka_predeploys(&mut tracking_copy).expect("upsert should succeed");

        assert_predeploy_present(&mut tracking_copy, EvmContract::eip4788());
        assert_predeploy_present(&mut tracking_copy, EvmContract::eip2935());
    }

    #[test]
    fn upsert_repairs_missing_eip2935_bytecode() {
        let predeploy = EvmContract::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            predeploy
                .code_hash_value()
                .expect("code hash value should build"),
        )]);

        upsert_osaka_predeploys(&mut tracking_copy).expect("upsert should succeed");

        assert_predeploy_present(&mut tracking_copy, predeploy);
    }

    #[test]
    fn upsert_noops_when_osaka_predeploys_are_present() {
        let (mut tracking_copy, _tempdir) =
            tracking_copy(osaka_predeploy_entries().expect("entries should build"));

        upsert_osaka_predeploys(&mut tracking_copy).expect("upsert should succeed");
    }

    #[test]
    fn upsert_rejects_conflicting_eip2935_non_empty_code_hash() {
        let predeploy = EvmContract::eip2935();
        let conflicting_hash = evm::Hash::new([1; evm::HASH_LENGTH]);
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            StoredValue::CLValue(CLValue::from_t(conflicting_hash).expect("hash should encode")),
        )]);

        let error = upsert_osaka_predeploys(&mut tracking_copy)
            .expect_err("conflicting code hash should fail");

        assert!(matches!(
            error,
            EvmContractError::ConflictingCodeHash {
                actual, ..
            } if actual == conflicting_hash
        ));
    }

    #[test]
    fn upsert_rejects_conflicting_eip2935_bytecode() {
        let predeploy = EvmContract::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([
            (
                predeploy.code_hash_key(),
                predeploy
                    .code_hash_value()
                    .expect("code hash value should build"),
            ),
            (
                predeploy.byte_code_key(),
                StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmOsaka, vec![0xfe])),
            ),
        ]);

        let error = upsert_osaka_predeploys(&mut tracking_copy)
            .expect_err("conflicting bytecode should fail");

        assert!(matches!(
            error,
            EvmContractError::ConflictingByteCode { .. }
        ));
    }

    #[test]
    fn upsert_repairs_empty_eip2935_code_hash() {
        let predeploy = EvmContract::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            StoredValue::CLValue(
                CLValue::from_t(evm::EMPTY_CODE_HASH).expect("hash should encode"),
            ),
        )]);

        upsert_osaka_predeploys(&mut tracking_copy).expect("upsert should succeed");

        assert_predeploy_present(&mut tracking_copy, predeploy);
    }

    #[test]
    fn upsert_rejects_unexpected_eip2935_code_hash_value() {
        let predeploy = EvmContract::eip2935();
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            predeploy.code_hash_key(),
            StoredValue::CLValue(
                CLValue::from_t(Key::Evm(EvmAddr::Account(
                    eip2935::BLOCK_HASH_HISTORY_ADDRESS,
                )))
                .expect("key should encode"),
            ),
        )]);

        let error = upsert_osaka_predeploys(&mut tracking_copy)
            .expect_err("invalid code hash value should fail");

        assert!(matches!(error, EvmContractError::CLValue(_)));
    }

    #[test]
    fn empty_preinstall_map_does_not_write_state() {
        let (mut tracking_copy, _tempdir) = tracking_copy([]);
        upsert_preinstalls(&mut tracking_copy, &BTreeMap::new()).unwrap();
        let (writes, prunes, _) = tracking_copy.destructure();
        assert!(writes.is_empty());
        assert!(prunes.is_empty());
    }

    #[test]
    fn empty_runtime_bytecode_is_rejected() {
        let address = evm::Address::new([1; 20]);
        let preinstalls = BTreeMap::from([(address, Bytes::new())]);
        let (mut tracking_copy, _tempdir) = tracking_copy([]);
        let error = upsert_preinstalls(&mut tracking_copy, &preinstalls).unwrap_err();
        assert!(
            matches!(error, EvmContractError::EmptyPreinstall { address: actual } if actual == address)
        );
        let (writes, prunes, _) = tracking_copy.destructure();
        assert!(writes.is_empty());
        assert!(prunes.is_empty());
    }

    #[test]
    fn upsert_creates_missing_preinstalls_without_predeploys() {
        let (mut tracking_copy, _tempdir) = tracking_copy([]);

        upsert_preinstalls(&mut tracking_copy, &test_preinstalls())
            .expect("preinstall upsert should succeed");

        for preinstall in test_contracts() {
            assert_predeploy_present(&mut tracking_copy, preinstall);
        }
        for predeploy in osaka_predeploys() {
            assert!(tracking_copy
                .read(&predeploy.code_hash_key())
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn upsert_preinstalls_repairs_empty_hash_and_missing_bytecode() {
        let contract = test_contracts()[0];
        for hash in [
            evm::EMPTY_CODE_HASH,
            evm::Hash::new([0; 32]),
            contract.code_hash,
        ] {
            let (mut tracking_copy, _tempdir) = tracking_copy([(
                contract.code_hash_key(),
                StoredValue::CLValue(CLValue::from_t(hash).unwrap()),
            )]);

            upsert_preinstalls(&mut tracking_copy, &test_preinstalls())
                .expect("preinstall repair should succeed");

            assert_predeploy_present(&mut tracking_copy, contract);
        }
    }

    #[test]
    fn upsert_preinstalls_noops_when_canonical_code_is_present() {
        let entries = test_contracts()
            .iter()
            .flat_map(|preinstall| predeploy_entries(*preinstall).unwrap())
            .collect::<std::collections::BTreeMap<_, _>>();
        let (mut tracking_copy, _tempdir) = tracking_copy(entries);

        upsert_preinstalls(&mut tracking_copy, &test_preinstalls())
            .expect("preinstall upsert should succeed");

        let (writes, prunes, _) = tracking_copy.destructure();
        assert!(writes.is_empty());
        assert!(prunes.is_empty());
    }

    #[test]
    fn upsert_preinstalls_preserves_account_metadata_balance_and_storage() {
        let preserved = test_contracts()
            .iter()
            .flat_map(|preinstall| {
                let contract = *preinstall;
                let purse = evm::deterministic_purse(contract.address);
                vec![
                    (
                        Key::Evm(EvmAddr::Account(contract.address)),
                        StoredValue::CLValue(CLValue::from_t(Key::URef(purse)).unwrap()),
                    ),
                    (
                        Key::Evm(EvmAddr::Nonce(contract.address)),
                        StoredValue::CLValue(CLValue::from_t(9u64).unwrap()),
                    ),
                    (
                        Key::Balance(purse.addr()),
                        StoredValue::CLValue(
                            CLValue::from_t(casper_types::U512::from(123)).unwrap(),
                        ),
                    ),
                    (
                        Key::Evm(EvmAddr::Storage(evm::StorageAddr::new(
                            contract.address,
                            casper_types::U256::from(7),
                        ))),
                        StoredValue::CLValue(
                            CLValue::from_t(casper_types::U256::from(42)).unwrap(),
                        ),
                    ),
                ]
            })
            .collect::<Vec<_>>();
        let (mut tracking_copy, _tempdir) = tracking_copy(preserved.clone());

        upsert_preinstalls(&mut tracking_copy, &test_preinstalls())
            .expect("preinstall upsert should succeed");

        for preinstall in test_contracts() {
            assert_predeploy_present(&mut tracking_copy, preinstall);
        }
        for (key, value) in preserved {
            assert_eq!(read(&mut tracking_copy, &key), value);
        }
        let (writes, prunes, _) = tracking_copy.destructure();
        let expected = test_contracts()
            .iter()
            .flat_map(|preinstall| predeploy_entries(*preinstall).unwrap())
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            writes
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>(),
            expected
        );
        assert!(prunes.is_empty());
    }

    #[test]
    fn upsert_preinstalls_rejects_conflicting_code_hash() {
        let contract = test_contracts()[0];
        let (mut tracking_copy, _tempdir) = tracking_copy([(
            contract.code_hash_key(),
            StoredValue::CLValue(CLValue::from_t(evm::Hash::new([1; 32])).unwrap()),
        )]);

        let error = upsert_preinstalls(&mut tracking_copy, &test_preinstalls())
            .expect_err("conflict should fail");

        assert!(
            matches!(error, EvmContractError::ConflictingCodeHash { address, name, .. }
            if address == contract.address && name == "EVM preinstall")
        );
        let (writes, prunes, _) = tracking_copy.destructure();
        assert!(writes.is_empty());
        assert!(prunes.is_empty());
    }

    #[test]
    fn upsert_preinstalls_rejects_conflicting_bytecode_or_kind() {
        let contract = test_contracts()[0];
        for bytecode in [
            ByteCode::new(ByteCodeKind::EvmOsaka, vec![0xfe]),
            ByteCode::new(ByteCodeKind::V1CasperWasm, contract.code.to_vec()),
        ] {
            let (mut tracking_copy, _tempdir) = tracking_copy([
                (
                    contract.code_hash_key(),
                    contract.code_hash_value().unwrap(),
                ),
                (contract.byte_code_key(), StoredValue::ByteCode(bytecode)),
            ]);

            let error = upsert_preinstalls(&mut tracking_copy, &test_preinstalls())
                .expect_err("conflict should fail");

            assert!(matches!(
                error,
                EvmContractError::ConflictingByteCode { .. }
            ));
            let (writes, prunes, _) = tracking_copy.destructure();
            assert!(writes.is_empty());
            assert!(prunes.is_empty());
        }
    }

    #[test]
    fn upsert_preinstalls_rejects_malformed_code_records() {
        let contract = test_contracts()[0];
        for key in [contract.code_hash_key(), contract.byte_code_key()] {
            let (mut tracking_copy, _tempdir) =
                tracking_copy([(key, StoredValue::CLValue(CLValue::from_t(42u64).unwrap()))]);

            upsert_preinstalls(&mut tracking_copy, &test_preinstalls())
                .expect_err("malformed record should fail");
        }
    }
}
