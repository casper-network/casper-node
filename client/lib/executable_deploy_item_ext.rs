use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_types::{bytesrepr::ToBytes, ContractPackageHash, RuntimeArgs};

use crate::error::Result;

/// Extension trait for `ExecutableDeployItem`, containing convenience constructors.
pub trait ExecutableDeployItemExt {
    /// Creates an `ExecutableDeployItem::StoredContractByName`.
    fn new_stored_contract_by_name(
        name: String,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem>;

    /// Creates an `ExecutableDeployItem::StoredContractByHash`.
    fn new_stored_contract_by_hash(
        hash: ContractPackageHash,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem>;

    /// Creates an `ExecutableDeployItem::StoredVersionedContractByName`.
    fn new_stored_versioned_contract_by_name(
        name: String,
        version: Option<u32>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem>;

    /// Creates an `ExecutableDeployItem::StoredVersionedContractByHash`.
    fn new_stored_versioned_contract_by_hash(
        hash: ContractPackageHash,
        version: Option<u32>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem>;

    /// Creates an `ExecutableDeployItem::ModuleBytes`.
    fn new_module_bytes(module_bytes: Vec<u8>, args: RuntimeArgs) -> Result<ExecutableDeployItem>;
}

impl ExecutableDeployItemExt for ExecutableDeployItem {
    fn new_stored_contract_by_name(
        name: String,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem> {
        Ok(ExecutableDeployItem::StoredContractByName {
            name,
            entry_point,
            args: args.to_bytes()?.into(),
        })
    }

    fn new_stored_contract_by_hash(
        hash: ContractPackageHash,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem> {
        Ok(ExecutableDeployItem::StoredContractByHash {
            hash,
            entry_point,
            args: args.to_bytes()?.into(),
        })
    }

    fn new_stored_versioned_contract_by_name(
        name: String,
        version: Option<u32>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem> {
        Ok(ExecutableDeployItem::StoredVersionedContractByName {
            name,
            version, // defaults to highest enabled version
            entry_point,
            args: args.to_bytes()?.into(),
        })
    }

    fn new_stored_versioned_contract_by_hash(
        hash: ContractPackageHash,
        version: Option<u32>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Result<ExecutableDeployItem> {
        Ok(ExecutableDeployItem::StoredVersionedContractByHash {
            hash,
            version, // defaults to highest enabled version
            entry_point,
            args: args.to_bytes()?.into(),
        })
    }

    fn new_module_bytes(module_bytes: Vec<u8>, args: RuntimeArgs) -> Result<ExecutableDeployItem> {
        Ok(ExecutableDeployItem::ModuleBytes {
            module_bytes: module_bytes.into(),
            args: args.to_bytes()?.into(),
        })
    }
}
