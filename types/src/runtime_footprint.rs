use crate::{
    account::AccountHash,
    addressable_entity::{AssociatedKeys, ContractRuntimeTag, Weight},
    contracts::{ContractHash, NamedKeys},
    system::SystemEntityType,
    Account, AddressableEntity, ContextAccessRights, Contract, EntityAddr, EntityKind, EntryPoints,
    HashAddr, Key, ProtocolVersion, URef,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
};
use core::{fmt::Debug, iter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const NAME_FOR_V2_CONTRACT_MAIN_PURSE: &str = "__main_purse";

/// Runtime Address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum RuntimeAddress {
    /// Account address
    Hash(HashAddr),
    /// Runtime executable address.
    StoredContract {
        /// The hash addr of the runtime entity
        hash_addr: HashAddr,
        /// The package hash
        package_hash_addr: HashAddr,
        /// The wasm hash
        wasm_hash_addr: HashAddr,
        /// protocol version
        protocol_version: ProtocolVersion,
    },
}

impl RuntimeAddress {
    /// Returns a new hash
    pub fn new_hash(hash_addr: HashAddr) -> Self {
        Self::Hash(hash_addr)
    }

    /// Returns new stored contract
    pub fn new_stored_contract(
        hash_addr: HashAddr,
        package_hash_addr: HashAddr,
        wasm_hash_addr: HashAddr,
        protocol_version: ProtocolVersion,
    ) -> Self {
        Self::StoredContract {
            hash_addr,
            package_hash_addr,
            wasm_hash_addr,
            protocol_version,
        }
    }

    /// The hash addr for the runtime.
    pub fn hash_addr(&self) -> HashAddr {
        match self {
            RuntimeAddress::Hash(hash_addr) => *hash_addr,
            RuntimeAddress::StoredContract { hash_addr, .. } => *hash_addr,
        }
    }
}

#[repr(u8)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Action {
    KeyManagement = 0,
    DeployManagement,
    UpgradeManagement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct RuntimeFootprint {
    named_keys: NamedKeys,
    action_thresholds: BTreeMap<u8, Weight>,
    associated_keys: AssociatedKeys,
    entry_points: EntryPoints,
    entity_kind: EntityKind,

    main_purse: Option<URef>,
    runtime_address: RuntimeAddress,
}

impl RuntimeFootprint {
    pub fn new(
        named_keys: NamedKeys,
        action_thresholds: BTreeMap<u8, Weight>,
        associated_keys: AssociatedKeys,
        entry_points: EntryPoints,
        entity_kind: EntityKind,
        main_purse: Option<URef>,
        runtime_address: RuntimeAddress,
    ) -> Self {
        Self {
            named_keys,
            action_thresholds,
            associated_keys,
            entry_points,
            entity_kind,
            main_purse,
            runtime_address,
        }
    }

    pub fn new_account_footprint(account: Account) -> Self {
        let named_keys = account.named_keys().clone();
        let action_thresholds = {
            let mut ret = BTreeMap::new();
            ret.insert(
                Action::KeyManagement as u8,
                Weight::new(account.action_thresholds().key_management.value()),
            );
            ret.insert(
                Action::DeployManagement as u8,
                Weight::new(account.action_thresholds().deployment.value()),
            );
            ret
        };
        let associated_keys = account.associated_keys().clone().into();
        let entry_points = EntryPoints::new();
        let entity_kind = EntityKind::Account(account.account_hash());
        let main_purse = Some(account.main_purse());
        let runtime_address = RuntimeAddress::new_hash(account.account_hash().value());

        Self::new(
            named_keys,
            action_thresholds,
            associated_keys,
            entry_points,
            entity_kind,
            main_purse,
            runtime_address,
        )
    }

    pub fn new_vm1_contract_footprint(
        contract_hash: ContractHash,
        contract: Contract,
        system_entity_type: Option<SystemEntityType>,
    ) -> Self {
        let contract_package_hash = contract.contract_package_hash();
        let contract_wasm_hash = contract.contract_wasm_hash();
        let entry_points = contract.entry_points().clone().into();
        let protocol_version = contract.protocol_version();
        let named_keys = contract.take_named_keys();

        let runtime_address = RuntimeAddress::new_stored_contract(
            contract_hash.value(),
            contract_package_hash.value(),
            contract_wasm_hash.value(),
            protocol_version,
        );

        let main_purse = None;
        let action_thresholds = BTreeMap::new();
        let associated_keys = AssociatedKeys::empty_keys();

        let entity_kind = match system_entity_type {
            None => EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1),
            Some(kind) => EntityKind::System(kind),
        };

        Self::new(
            named_keys,
            action_thresholds,
            associated_keys,
            entry_points,
            entity_kind,
            main_purse,
            runtime_address,
        )
    }

    pub fn new_vm2_contract_footprint(
        contract_hash: ContractHash,
        contract: Contract,
        system_entity_type: Option<SystemEntityType>,
    ) -> Self {
        let contract_package_hash = contract.contract_package_hash();
        let contract_wasm_hash = contract.contract_wasm_hash();
        let entry_points = contract.entry_points().clone().into();
        let protocol_version = contract.protocol_version();
        let mut named_keys = contract.take_named_keys();

        let runtime_address = RuntimeAddress::new_stored_contract(
            contract_hash.value(),
            contract_package_hash.value(),
            contract_wasm_hash.value(),
            protocol_version,
        );

        let main_purse = {
            match named_keys
                .remove(NAME_FOR_V2_CONTRACT_MAIN_PURSE)
                .map(|key| key.into_uref())
            {
                Some(Some(uref)) => Some(uref),
                Some(_) | None => None,
            }
        };

        let action_thresholds = BTreeMap::new();
        let associated_keys = AssociatedKeys::empty_keys();

        let entity_kind = match system_entity_type {
            None => EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2),
            Some(kind) => EntityKind::System(kind),
        };

        Self::new(
            named_keys,
            action_thresholds,
            associated_keys,
            entry_points,
            entity_kind,
            main_purse,
            runtime_address,
        )
    }

    pub fn new_entity_footprint(
        entity_addr: EntityAddr,
        entity: AddressableEntity,
        named_keys: NamedKeys,
        entry_points: EntryPoints,
    ) -> Self {
        let runtime_address = RuntimeAddress::new_stored_contract(
            entity_addr.value(),
            entity.package().value(),
            entity.byte_code().value(),
            entity.protocol_version(),
        );
        let action_thresholds = {
            let mut ret = BTreeMap::new();
            ret.insert(
                Action::KeyManagement as u8,
                entity.action_thresholds().key_management,
            );
            ret.insert(
                Action::DeployManagement as u8,
                entity.action_thresholds().key_management,
            );
            ret.insert(
                Action::UpgradeManagement as u8,
                entity.action_thresholds().upgrade_management,
            );
            ret
        };
        Self::new(
            named_keys,
            action_thresholds,
            entity.associated_keys().clone(),
            entry_points,
            entity.entity_kind(),
            Some(entity.main_purse()),
            runtime_address,
        )
    }

    pub fn package_hash(&self) -> Option<HashAddr> {
        match &self.runtime_address {
            RuntimeAddress::Hash(_) => None,
            RuntimeAddress::StoredContract {
                package_hash_addr, ..
            } => Some(*package_hash_addr),
        }
    }

    pub fn associated_keys(&self) -> &AssociatedKeys {
        &self.associated_keys
    }

    pub fn wasm_hash(&self) -> Option<HashAddr> {
        match &self.runtime_address {
            RuntimeAddress::Hash(_) => None,
            RuntimeAddress::StoredContract { wasm_hash_addr, .. } => Some(*wasm_hash_addr),
        }
    }

    pub fn hash_addr(&self) -> HashAddr {
        match &self.runtime_address {
            RuntimeAddress::Hash(hash_addr) => *hash_addr,
            RuntimeAddress::StoredContract { hash_addr, .. } => *hash_addr,
        }
    }

    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }

    pub fn insert_into_named_keys(&mut self, name: String, key: Key) {
        self.named_keys.insert(name, key);
    }

    pub fn named_keys_mut(&mut self) -> &mut NamedKeys {
        &mut self.named_keys
    }

    pub fn take_named_keys(self) -> NamedKeys {
        self.named_keys
    }

    pub fn main_purse(&self) -> Option<URef> {
        self.main_purse
    }

    pub fn set_main_purse(&mut self, purse: URef) {
        self.main_purse = Some(purse);
    }

    pub fn entry_points(&self) -> &EntryPoints {
        &self.entry_points
    }

    pub fn entity_kind(&self) -> EntityKind {
        self.entity_kind
    }

    /// Checks whether all authorization keys are associated with this addressable entity.
    pub fn can_authorize(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        !authorization_keys.is_empty()
            && authorization_keys
                .iter()
                .any(|e| self.associated_keys.contains_key(e))
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to key management threshold.
    pub fn can_manage_keys_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        match self.action_thresholds.get(&(Action::KeyManagement as u8)) {
            None => false,
            Some(weight) => total_weight >= *weight,
        }
    }

    /// Checks whether the sum of the weights of all authorization keys is
    /// greater or equal to deploy threshold.
    pub fn can_deploy_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        match self
            .action_thresholds
            .get(&(Action::DeployManagement as u8))
        {
            None => false,
            Some(weight) => total_weight >= *weight,
        }
    }

    pub fn can_upgrade_with(&self, authorization_keys: &BTreeSet<AccountHash>) -> bool {
        let total_weight = self
            .associated_keys
            .calculate_keys_weight(authorization_keys);

        match self
            .action_thresholds
            .get(&(Action::UpgradeManagement as u8))
        {
            None => false,
            Some(weight) => total_weight >= *weight,
        }
    }

    /// Extracts the access rights from the named keys of the addressable entity.
    pub fn extract_access_rights(&self) -> ContextAccessRights {
        match self.main_purse {
            Some(purse) => {
                let urefs_iter = self
                    .named_keys
                    .keys()
                    .filter_map(|key| key.as_uref().copied())
                    .chain(iter::once(purse));
                ContextAccessRights::new(self.hash_addr(), urefs_iter)
            }
            None => {
                let urefs_iter = self
                    .named_keys
                    .keys()
                    .filter_map(|key| key.as_uref().copied());
                ContextAccessRights::new(self.hash_addr(), urefs_iter)
            }
        }
    }
}
