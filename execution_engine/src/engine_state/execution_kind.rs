//! Units of execution.

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyExt},
};
use casper_types::{
    bytesrepr::Bytes,
    contracts::{NamedKeys, ProtocolVersionMajor},
    AddressableEntityHash, EntityVersion, Key, PackageHash, StoredValue,
    TransactionInvocationTarget,
};

use super::{wasm_v1::SessionKind, Error, ExecutableItem};
use crate::execution::ExecError;

/// The type of execution about to be performed.
#[derive(Clone, Debug)]
pub(crate) enum ExecutionKind<'a> {
    /// Standard (non-specialized) Wasm bytes related to a transaction of version 1 or later.
    Standard(&'a Bytes),
    /// Wasm bytes which install or upgrade a stored entity.
    InstallerUpgrader(&'a Bytes),
    /// Stored contract.
    Stored {
        /// AddressableEntity's hash.
        entity_hash: AddressableEntityHash,
        /// Entry point.
        entry_point: String,
    },
    /// Standard (non-specialized) Wasm bytes related to a `Deploy`.
    ///
    /// This is equivalent to the `Standard` variant with the exception that this kind will be
    /// allowed to install or upgrade stored entities to retain existing (pre-node 2.0) behavior.
    Deploy(&'a Bytes),
    /// A call to an entity/contract in a package/contract package.
    VersionedCall {
        package_hash: PackageHash,
        entity_version: Option<EntityVersion>,
        protocol_version_major: Option<ProtocolVersionMajor>,
        /// Entry point.
        entry_point: String,
    },
}

impl<'a> ExecutionKind<'a> {
    pub(crate) fn new<R>(
        tracking_copy: &mut TrackingCopy<R>,
        named_keys: &NamedKeys,
        executable_item: &'a ExecutableItem,
        entry_point: String,
    ) -> Result<Self, Error>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        match executable_item {
            ExecutableItem::Invocation(target) => {
                Self::new_direct_invocation(tracking_copy, named_keys, target, entry_point)
            }
            ExecutableItem::PaymentBytes(module_bytes)
            | ExecutableItem::SessionBytes {
                kind: SessionKind::GenericBytecode,
                module_bytes,
            } => Ok(ExecutionKind::Standard(module_bytes)),
            ExecutableItem::SessionBytes {
                kind: SessionKind::InstallUpgradeBytecode,
                module_bytes,
            } => Ok(ExecutionKind::InstallerUpgrader(module_bytes)),
            ExecutableItem::Deploy(module_bytes) => Ok(ExecutionKind::Deploy(module_bytes)),
        }
    }

    fn new_direct_invocation<R>(
        tracking_copy: &mut TrackingCopy<R>,
        named_keys: &NamedKeys,
        target: &TransactionInvocationTarget,
        entry_point: String,
    ) -> Result<Self, Error>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let entity_hash = match target {
            TransactionInvocationTarget::ByHash(addr) => AddressableEntityHash::new(*addr),
            TransactionInvocationTarget::ByName(alias) => {
                let entity_key = named_keys
                    .get(alias)
                    .ok_or_else(|| Error::Exec(ExecError::NamedKeyNotFound(alias.clone())))?;

                match entity_key {
                    Key::Hash(hash) => AddressableEntityHash::new(*hash),
                    Key::AddressableEntity(entity_addr) => {
                        AddressableEntityHash::new(entity_addr.value())
                    }
                    _ => return Err(Error::InvalidKeyVariant(*entity_key)),
                }
            }
            TransactionInvocationTarget::ByPackageHash {
                addr,
                version_key,
                version, // version is defunct and should not be used
            } => {
                let protocol_version_major = version_key.map(|vk| vk.protocol_version_major());

                let package_hash = PackageHash::from(*addr);
                return Ok(Self::VersionedCall {
                    package_hash,
                    entity_version: *version,
                    protocol_version_major,
                    entry_point,
                });
            }
            TransactionInvocationTarget::ByPackageName {
                name: alias,
                version_key,
                version, // version is defunct and should not be used
            } => {
                let package_key = named_keys
                    .get(alias)
                    .ok_or_else(|| Error::Exec(ExecError::NamedKeyNotFound(alias.to_string())))?;

                let package_hash = match package_key {
                    Key::Hash(hash) | Key::SmartContract(hash) => PackageHash::new(*hash),
                    _ => return Err(Error::InvalidKeyVariant(*package_key)),
                };
                let protocol_version_major = version_key.map(|vk| vk.protocol_version_major());
                return Ok(Self::VersionedCall {
                    package_hash,
                    entity_version: *version,
                    protocol_version_major,
                    entry_point,
                });
            }
        };
        Ok(ExecutionKind::Stored {
            entity_hash,
            entry_point,
        })
    }
}
