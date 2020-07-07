use super::error;
use crate::contract_core::execution;
use crate::contract_shared::account::Account;
use types::{
    bytesrepr,
    contracts::{ContractVersion, DEFAULT_ENTRY_POINT_NAME},
    ContractHash, ContractPackageHash, Key, RuntimeArgs,
};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExecutableDeployItem {
    ModuleBytes {
        module_bytes: Vec<u8>,
        // assumes implicit `call` noarg entrypoint
        args: Vec<u8>,
    },
    StoredContractByHash {
        hash: ContractHash,
        entry_point: String,
        args: Vec<u8>,
    },
    StoredContractByName {
        name: String,
        entry_point: String,
        args: Vec<u8>,
    },
    StoredVersionedContractByName {
        name: String,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        args: Vec<u8>,
    },
    StoredVersionedContractByHash {
        hash: ContractPackageHash,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        args: Vec<u8>,
    },
    Transfer {
        args: Vec<u8>,
    },
}

impl ExecutableDeployItem {
    pub(crate) fn to_contract_hash_key(
        &self,
        account: &Account,
    ) -> Result<Option<Key>, error::Error> {
        match self {
            ExecutableDeployItem::StoredContractByHash { hash, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { hash, .. } => {
                Ok(Some(Key::from(*hash)))
            }
            ExecutableDeployItem::StoredContractByName { name, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { name, .. } => {
                let key = account.named_keys().get(name).cloned().ok_or_else(|| {
                    error::Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
                })?;
                Ok(Some(key))
            }
            ExecutableDeployItem::ModuleBytes { .. } | ExecutableDeployItem::Transfer { .. } => {
                Ok(None)
            }
        }
    }

    pub fn into_runtime_args(self) -> Result<RuntimeArgs, bytesrepr::Error> {
        match self {
            ExecutableDeployItem::ModuleBytes { args, .. }
            | ExecutableDeployItem::StoredContractByHash { args, .. }
            | ExecutableDeployItem::StoredContractByName { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { args, .. }
            | ExecutableDeployItem::Transfer { args } => {
                let runtime_args: RuntimeArgs = bytesrepr::deserialize(args)?;
                Ok(runtime_args)
            }
        }
    }

    pub fn entry_point_name(&self) -> &str {
        match self {
            ExecutableDeployItem::ModuleBytes { .. } | ExecutableDeployItem::Transfer { .. } => {
                DEFAULT_ENTRY_POINT_NAME
            }
            ExecutableDeployItem::StoredVersionedContractByName { entry_point, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { entry_point, .. }
            | ExecutableDeployItem::StoredContractByHash { entry_point, .. }
            | ExecutableDeployItem::StoredContractByName { entry_point, .. } => &entry_point,
        }
    }
}
