//! Units of execution.

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyExt},
};
use casper_types::{
    addressable_entity::NamedKeys, bytesrepr::Bytes, AddressableEntityHash, EntityVersionKey, Key,
    PackageHash, ProtocolVersion, StoredValue, TransactionInvocationTarget,
};

use crate::{
    engine_state::error::Error,
    execution::{self},
};

/// The type of execution about to be performed.
#[derive(Clone, Debug)]
pub(crate) enum ExecutionKind<'a> {
    /// Standard (non-specialized) Wasm bytes.
    Standard(&'a Bytes),
    /// Wasm bytes which install a stored entity.
    Installer(&'a Bytes),
    /// Wasm bytes which upgrade a stored entity.
    Upgrader(&'a Bytes),
    /// Wasm bytes which don't call any stored entity.
    Isolated(&'a Bytes),
    /// Stored contract.
    Stored {
        /// AddressableEntity's hash.
        entity_hash: AddressableEntityHash,
        /// Entry point.
        entry_point: String,
    },
}

impl<'a> ExecutionKind<'a> {
    /// Returns a new `Standard` variant of `ExecutionKind`.
    pub fn new_standard(module_bytes: &'a Bytes) -> Self {
        ExecutionKind::Standard(module_bytes)
    }

    /// Returns a new `Installer` variant of `ExecutionKind`.
    pub fn new_installer(module_bytes: &'a Bytes) -> Self {
        ExecutionKind::Installer(module_bytes)
    }

    /// Returns a new `Upgrader` variant of `ExecutionKind`.
    pub fn new_upgrader(module_bytes: &'a Bytes) -> Self {
        ExecutionKind::Upgrader(module_bytes)
    }

    /// Returns a new `Isolated` variant of `ExecutionKind`.
    pub fn new_isolated(module_bytes: &'a Bytes) -> Self {
        ExecutionKind::Isolated(module_bytes)
    }

    /// Returns a new `Standard` variant of `ExecutionKind`, returning an error if the module bytes
    /// are empty.
    pub fn new_for_payment(module_bytes: &'a Bytes) -> Result<Self, Error> {
        if module_bytes.is_empty() {
            return Err(Error::EmptyCustomPaymentModuleBytes);
        }
        Ok(ExecutionKind::Standard(module_bytes))
    }

    /// Returns a new `ExecutionKind` cloned from `self`, but converting any Wasm bytes variant to
    /// `Standard`, and returning an error if they are empty.
    pub fn convert_for_payment(&self) -> Result<Self, Error> {
        match self {
            ExecutionKind::Standard(module_bytes)
            | ExecutionKind::Installer(module_bytes)
            | ExecutionKind::Upgrader(module_bytes)
            | ExecutionKind::Isolated(module_bytes) => Self::new_for_payment(module_bytes),
            ExecutionKind::Stored { .. } => Ok(self.clone()),
        }
    }

    /// Returns a new contract variant of `ExecutionKind`.
    pub fn new_stored<R>(
        tracking_copy: &mut TrackingCopy<R>,
        target: TransactionInvocationTarget,
        entry_point: String,
        named_keys: &NamedKeys,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, Error>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let entity_hash = match target {
            TransactionInvocationTarget::InvocableEntity(addr) => AddressableEntityHash::new(addr),
            TransactionInvocationTarget::InvocableEntityAlias(alias) => {
                let entity_key = named_keys
                    .get(&alias)
                    .cloned()
                    .ok_or_else(|| Error::Exec(execution::Error::NamedKeyNotFound(alias)))?;

                match entity_key {
                    Key::Hash(hash) => AddressableEntityHash::new(hash),
                    Key::AddressableEntity(entity_addr) => {
                        AddressableEntityHash::new(entity_addr.value())
                    }
                    _ => return Err(Error::InvalidKeyVariant),
                }
            }
            TransactionInvocationTarget::Package { addr, version } => {
                let package_hash = PackageHash::from(addr);
                let package = tracking_copy.get_package(package_hash)?;

                let maybe_version_key =
                    version.map(|ver| EntityVersionKey::new(protocol_version.value().major, ver));

                let contract_version_key = maybe_version_key
                    .or_else(|| package.current_entity_version())
                    .ok_or(Error::Exec(execution::Error::NoActiveEntityVersions(
                        package_hash,
                    )))?;

                if !package.is_version_enabled(contract_version_key) {
                    return Err(Error::Exec(execution::Error::InvalidEntityVersion(
                        contract_version_key,
                    )));
                }

                *package
                    .lookup_entity_hash(contract_version_key)
                    .ok_or(Error::Exec(execution::Error::InvalidEntityVersion(
                        contract_version_key,
                    )))?
            }
            TransactionInvocationTarget::PackageAlias { alias, version } => {
                let package_key = named_keys.get(&alias).cloned().ok_or_else(|| {
                    Error::Exec(execution::Error::NamedKeyNotFound(alias.to_string()))
                })?;

                let package_hash = match package_key {
                    Key::Hash(hash) | Key::Package(hash) => PackageHash::new(hash),
                    _ => return Err(Error::InvalidKeyVariant),
                };

                let package = tracking_copy.get_package(package_hash)?;

                let maybe_version_key =
                    version.map(|ver| EntityVersionKey::new(protocol_version.value().major, ver));

                let contract_version_key = maybe_version_key
                    .or_else(|| package.current_entity_version())
                    .ok_or(Error::Exec(execution::Error::NoActiveEntityVersions(
                        package_hash,
                    )))?;

                if !package.is_version_enabled(contract_version_key) {
                    return Err(Error::Exec(execution::Error::InvalidEntityVersion(
                        contract_version_key,
                    )));
                }

                *package
                    .lookup_entity_hash(contract_version_key)
                    .ok_or(Error::Exec(execution::Error::InvalidEntityVersion(
                        contract_version_key,
                    )))?
            }
        };
        Ok(ExecutionKind::Stored {
            entity_hash,
            entry_point,
        })
    }
}
