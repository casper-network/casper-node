use std::convert::{TryFrom, TryInto};

use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;

use crate::engine_server::{
    ipc::{DeployPayload, DeployPayload_oneof_payload},
    mappings::MappingError,
};

impl TryFrom<DeployPayload_oneof_payload> for ExecutableDeployItem {
    type Error = MappingError;
    fn try_from(pb_deploy_payload: DeployPayload_oneof_payload) -> Result<Self, Self::Error> {
        Ok(match pb_deploy_payload {
            DeployPayload_oneof_payload::deploy_code(pb_deploy_code) => {
                ExecutableDeployItem::ModuleBytes {
                    module_bytes: pb_deploy_code.code.into(),
                    args: pb_deploy_code.args.into(),
                }
            }
            DeployPayload_oneof_payload::stored_contract_hash(mut pb_stored_contract_hash) => {
                let hash_bytes = pb_stored_contract_hash.take_hash();
                let hash = hash_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| MappingError::invalid_hash_length(hash_bytes.len()))?;
                ExecutableDeployItem::StoredContractByHash {
                    hash,
                    entry_point: pb_stored_contract_hash.entry_point_name,
                    args: pb_stored_contract_hash.args.into(),
                }
            }
            DeployPayload_oneof_payload::stored_contract_name(pb_stored_contract_name) => {
                ExecutableDeployItem::StoredContractByName {
                    name: pb_stored_contract_name.name,
                    entry_point: pb_stored_contract_name.entry_point_name,
                    args: pb_stored_contract_name.args.into(),
                }
            }
            DeployPayload_oneof_payload::stored_package_by_name(mut pb_stored_package_by_name) => {
                ExecutableDeployItem::StoredVersionedContractByName {
                    name: pb_stored_package_by_name.take_name(),
                    version: if pb_stored_package_by_name.has_version()
                        && pb_stored_package_by_name.get_version() > 0
                    {
                        Some(pb_stored_package_by_name.get_version())
                    } else {
                        None
                    },
                    entry_point: pb_stored_package_by_name.entry_point_name,
                    args: pb_stored_package_by_name.args.into(),
                }
            }
            DeployPayload_oneof_payload::stored_package_by_hash(mut pb_stored_package_by_hash) => {
                let hash_bytes = pb_stored_package_by_hash.take_hash();
                let hash = hash_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| MappingError::invalid_hash_length(hash_bytes.len()))?;
                ExecutableDeployItem::StoredVersionedContractByHash {
                    hash,
                    version: if pb_stored_package_by_hash.has_version()
                        && pb_stored_package_by_hash.get_version() > 0
                    {
                        Some(pb_stored_package_by_hash.get_version())
                    } else {
                        None
                    },
                    entry_point: pb_stored_package_by_hash.entry_point_name,
                    args: pb_stored_package_by_hash.args.into(),
                }
            }
            DeployPayload_oneof_payload::transfer(pb_transfer) => ExecutableDeployItem::Transfer {
                args: pb_transfer.args.into(),
            },
        })
    }
}

impl From<ExecutableDeployItem> for DeployPayload {
    fn from(edi: ExecutableDeployItem) -> Self {
        let mut result = DeployPayload::new();
        match edi {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                let code = result.mut_deploy_code();
                code.set_code(module_bytes.into());
                code.set_args(args.into());
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                let inner = result.mut_stored_contract_hash();
                inner.set_hash(hash.value().to_vec());
                inner.set_entry_point_name(entry_point);
                inner.set_args(args.into());
            }
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                let inner = result.mut_stored_contract_name();
                inner.set_name(name);
                inner.set_entry_point_name(entry_point);
                inner.set_args(args.into());
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => {
                let inner = result.mut_stored_package_by_name();
                inner.set_name(name);
                if let Some(ver) = version {
                    inner.set_version(ver)
                }
                inner.set_entry_point_name(entry_point);
                inner.set_args(args.into());
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                let inner = result.mut_stored_package_by_hash();
                inner.set_hash(hash.value().to_vec());
                if let Some(ver) = version {
                    inner.set_version(ver)
                }
                inner.set_entry_point_name(entry_point);
                inner.set_args(args.into());
            }
            ExecutableDeployItem::Transfer { args } => {
                let inner = result.mut_transfer();
                inner.set_args(args.into());
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_panic_for_invalid_hash() {
        let bad_hash = b"This string is definetely longer than 32 bytes";

        let mut deploy_payload = DeployPayload::new();

        let inner = deploy_payload.mut_stored_contract_hash();
        inner.set_hash(bad_hash.to_vec());
        inner.set_entry_point_name("EntryPoint".to_string());
        inner.set_args(b"".to_vec()); // Empty

        let err = ExecutableDeployItem::try_from(deploy_payload.payload.unwrap()).unwrap_err();
        assert_eq!(
            err,
            MappingError::InvalidHashLength {
                actual: bad_hash.len(),
                expected: 32
            }
        );
    }
}
