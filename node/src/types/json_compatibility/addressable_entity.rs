// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::json_compatibility::vectorize;
use casper_types::{
    account::AccountHash, addressable_entity::AddressableEntity as DomainEntity,
    ContractPackageHash, ContractWasmHash, EntryPoint, NamedKey, ProtocolVersion, URef,
};

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AssociatedKey {
    account_hash: AccountHash,
    weight: u8,
}

/// Thresholds that have to be met when executing an action of a certain type.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ActionThresholds {
    deployment: u8,
    key_management: u8,
}

/// A contract struct that can be serialized as  JSON object.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddressableEntity {
    contract_package_hash: ContractPackageHash,
    contract_wasm_hash: ContractWasmHash,
    #[data_size(skip)]
    named_keys: Vec<NamedKey>,
    #[data_size(skip)]
    entry_points: Vec<EntryPoint>,
    #[data_size(skip)]
    #[schemars(with = "String")]
    protocol_version: ProtocolVersion,
    #[data_size(skip)]
    main_purse: URef,
    associated_keys: Vec<AssociatedKey>,
    action_thresholds: ActionThresholds,
}

impl From<&DomainEntity> for AddressableEntity {
    fn from(entity: &DomainEntity) -> Self {
        let entry_points = entity.entry_points().clone().take_entry_points();
        Self {
            contract_package_hash: entity.contract_package_hash(),
            contract_wasm_hash: entity.contract_wasm_hash(),
            named_keys: vectorize(entity.named_keys()),
            entry_points,
            protocol_version: entity.protocol_version(),
            main_purse: entity.main_purse(),
            associated_keys: entity
                .associated_keys()
                .iter()
                .map(|(account_hash, weight)| AssociatedKey {
                    account_hash: *account_hash,
                    weight: weight.value(),
                })
                .collect(),
            action_thresholds: ActionThresholds {
                deployment: entity.action_thresholds().deployment().value(),
                key_management: entity.action_thresholds().key_management().value(),
            },
        }
    }
}
