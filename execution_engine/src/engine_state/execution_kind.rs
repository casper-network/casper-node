//! Units of execution.

use std::{cell::RefCell, rc::Rc};

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyExt},
};
use casper_types::{
    addressable_entity::NamedKeys, bytesrepr::Bytes, AddressableEntityHash, EntityVersionKey,
    ExecutableDeployItem, Key, Package, PackageHash, Phase, ProtocolVersion, StoredValue,
};

use crate::{
    engine_state::error::Error,
    execution::{self},
};

/// The type of execution about to be performed.
#[derive(Clone, Debug)]
pub(crate) enum ExecutionKind {
    /// Wasm bytes.
    Module(Bytes),
    /// Stored contract.
    Contract {
        /// AddressableEntity's hash.
        entity_hash: AddressableEntityHash,
        /// Entry point's name.
        entry_point_name: String,
    },
}

impl ExecutionKind {
    /// Returns a new module variant of `ExecutionKind`.
    pub fn new_module(module_bytes: Bytes) -> Self {
        ExecutionKind::Module(module_bytes)
    }

    /// Returns a new contract variant of `ExecutionKind`.
    pub fn new_addressable_entity(
        entity_hash: AddressableEntityHash,
        entry_point_name: String,
    ) -> Self {
        ExecutionKind::Contract {
            entity_hash,
            entry_point_name,
        }
    }

    /// Returns all the details necessary for execution.
    ///
    /// This object is generated based on information provided by [`ExecutableDeployItem`].
    pub fn new<R>(
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        named_keys: &NamedKeys,
        executable_deploy_item: ExecutableDeployItem,
        protocol_version: &ProtocolVersion,
        phase: Phase,
    ) -> Result<ExecutionKind, Error>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let package: Package;

        let is_payment_phase = phase == Phase::Payment;

        match executable_deploy_item {
            ExecutableDeployItem::Transfer { .. } => {
                Err(Error::InvalidDeployItemVariant("Transfer".into()))
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. }
                if module_bytes.is_empty() && is_payment_phase =>
            {
                Err(Error::InvalidDeployItemVariant(
                    "Empty module bytes for custom payment".into(),
                ))
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                Ok(ExecutionKind::new_module(module_bytes))
            }
            ExecutableDeployItem::StoredContractByHash {
                hash, entry_point, ..
            } => Ok(ExecutionKind::new_addressable_entity(hash, entry_point)),
            ExecutableDeployItem::StoredContractByName {
                name, entry_point, ..
            } => {
                let entity_key = named_keys.get(&name).cloned().ok_or_else(|| {
                    Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
                })?;

                let entity_hash = match entity_key {
                    Key::Hash(hash) => AddressableEntityHash::new(hash),
                    Key::AddressableEntity(entity_addr) => {
                        AddressableEntityHash::new(entity_addr.value())
                    }
                    _ => return Err(Error::InvalidKeyVariant),
                };

                Ok(ExecutionKind::new_addressable_entity(
                    entity_hash,
                    entry_point,
                ))
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                ..
            } => {
                let package_key = named_keys.get(&name).cloned().ok_or_else(|| {
                    Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
                })?;

                let package_hash = match package_key {
                    Key::Hash(hash) | Key::Package(hash) => PackageHash::new(hash),
                    _ => return Err(Error::InvalidKeyVariant),
                };

                package = tracking_copy.borrow_mut().get_package(package_hash)?;

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

                let looked_up_entity_hash: AddressableEntityHash = package
                    .lookup_entity_hash(contract_version_key)
                    .ok_or(Error::Exec(execution::Error::InvalidEntityVersion(
                        contract_version_key,
                    )))?
                    .to_owned();

                Ok(ExecutionKind::new_addressable_entity(
                    looked_up_entity_hash,
                    entry_point,
                ))
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash: package_hash,
                version,
                entry_point,
                ..
            } => {
                package = tracking_copy.borrow_mut().get_package(package_hash)?;

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

                let looked_up_entity_hash = *package
                    .lookup_entity_hash(contract_version_key)
                    .ok_or(Error::Exec(execution::Error::InvalidEntityVersion(
                        contract_version_key,
                    )))?;

                Ok(ExecutionKind::new_addressable_entity(
                    looked_up_entity_hash,
                    entry_point,
                ))
            }
        }
    }
}
