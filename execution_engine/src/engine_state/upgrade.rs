//! Support for applying upgrades on the execution engine.
use std::{cell::RefCell, collections::BTreeSet, fmt, rc::Rc};

use thiserror::Error;

use casper_storage::{
    global_state::state::StateProvider, tracking_copy::TrackingCopy, AddressGenerator,
};
use casper_types::{
    addressable_entity::{
        ActionThresholds, AssociatedKeys, EntityKind, EntityKindTag, MessageTopics, NamedKeyAddr,
        NamedKeyValue, NamedKeys, Weight,
    },
    bytesrepr::{self, ToBytes},
    execution::Effects,
    package::{EntityVersions, Groups, PackageStatus},
    system::{handle_payment::ACCUMULATION_PURSE_KEY, SystemEntityType},
    AccessRights, AddressableEntity, AddressableEntityHash, ByteCode, ByteCodeAddr, ByteCodeHash,
    ByteCodeKind, CLValue, CLValueError, Digest, EntityAddr, EntryPoints, FeeHandling, Key,
    Package, PackageHash, Phase, ProtocolVersion, PublicKey, StoredValue, URef, U512,
};

use super::EngineConfig;

const NO_PRUNE: bool = false;
const PRUNE: bool = true;

/// Represents a successfully executed upgrade.
#[derive(Debug, Clone)]
pub struct UpgradeSuccess {
    /// New state root hash generated after effects were applied.
    pub post_state_hash: Digest,
    /// Effects of executing an upgrade request.
    pub effects: Effects,
}

impl fmt::Display for UpgradeSuccess {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "Success: {} {:?}", self.post_state_hash, self.effects)
    }
}

/// Represents outcomes of a failed protocol upgrade.
#[derive(Clone, Error, Debug)]
pub enum ProtocolUpgradeError {
    /// Error validating a protocol upgrade config.
    #[error("Invalid upgrade config")]
    InvalidUpgradeConfig,
    /// Unable to retrieve a system contract.
    #[error("Unable to retrieve system contract: {0}")]
    UnableToRetrieveSystemContract(String),
    /// Unable to retrieve a system contract package.
    #[error("Unable to retrieve system contract package: {0}")]
    UnableToRetrieveSystemContractPackage(String),
    /// Unable to disable previous version of a system contract.
    #[error("Failed to disable previous version of system contract: {0}")]
    FailedToDisablePreviousVersion(String),
    /// (De)serialization error.
    #[error("{0}")]
    Bytesrepr(bytesrepr::Error),
    /// Failed to create system contract registry.
    #[error("Failed to insert system contract registry")]
    FailedToCreateSystemRegistry,
    /// Found unexpected variant of a stored value.
    #[error("Unexpected stored value variant")]
    UnexpectedStoredValueVariant,
    /// Failed to convert into a CLValue.
    #[error("{0}")]
    CLValue(String),
}

impl From<CLValueError> for ProtocolUpgradeError {
    fn from(v: CLValueError) -> Self {
        Self::CLValue(v.to_string())
    }
}

impl From<bytesrepr::Error> for ProtocolUpgradeError {
    fn from(error: bytesrepr::Error) -> Self {
        ProtocolUpgradeError::Bytesrepr(error)
    }
}

/// The system upgrader deals with conducting an actual protocol upgrade.
pub(crate) struct SystemUpgrader<S>
where
    S: StateProvider,
{
    new_protocol_version: ProtocolVersion,
    old_protocol_version: ProtocolVersion,
    tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
}

impl<S> SystemUpgrader<S>
where
    S: StateProvider,
{
    /// Creates new system upgrader instance.
    pub(crate) fn new(
        new_protocol_version: ProtocolVersion,
        old_protocol_version: ProtocolVersion,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Self {
        SystemUpgrader {
            new_protocol_version,
            old_protocol_version,
            tracking_copy,
        }
    }

    /// Bump major version and/or update the entry points for system contracts.
    pub(crate) fn refresh_system_contracts(
        &self,
        mint_hash: &AddressableEntityHash,
        auction_hash: &AddressableEntityHash,
        handle_payment_hash: &AddressableEntityHash,
    ) -> Result<(), ProtocolUpgradeError> {
        self.refresh_system_contract_entry_points(*mint_hash, SystemEntityType::Mint)?;
        self.refresh_system_contract_entry_points(*auction_hash, SystemEntityType::Auction)?;
        self.refresh_system_contract_entry_points(
            *handle_payment_hash,
            SystemEntityType::HandlePayment,
        )?;

        Ok(())
    }

    /// Refresh the system contracts with an updated set of entry points,
    /// and bump the contract version at a major version upgrade.
    fn refresh_system_contract_entry_points(
        &self,
        contract_hash: AddressableEntityHash,
        system_contract_type: SystemEntityType,
    ) -> Result<(), ProtocolUpgradeError> {
        let contract_name = system_contract_type.contract_name();
        let entry_points = system_contract_type.contract_entry_points();

        let (mut contract, maybe_named_keys, must_prune) =
            self.retrieve_system_contract(contract_hash, system_contract_type)?;

        let mut package =
            self.retrieve_system_package(contract.package_hash(), system_contract_type)?;

        package.disable_entity_version(contract_hash).map_err(|_| {
            ProtocolUpgradeError::FailedToDisablePreviousVersion(contract_name.to_string())
        })?;

        contract.set_protocol_version(self.new_protocol_version);

        let new_entity = AddressableEntity::new(
            contract.package_hash(),
            ByteCodeHash::default(),
            entry_points,
            self.new_protocol_version,
            URef::default(),
            AssociatedKeys::default(),
            ActionThresholds::default(),
            MessageTopics::default(),
            EntityKind::System(system_contract_type),
        );

        let byte_code_key = Key::byte_code_key(ByteCodeAddr::Empty);
        let byte_code = ByteCode::new(ByteCodeKind::Empty, vec![]);

        self.tracking_copy
            .borrow_mut()
            .write(byte_code_key, StoredValue::ByteCode(byte_code));

        let entity_key = new_entity.entity_key(contract_hash);

        self.tracking_copy
            .borrow_mut()
            .write(entity_key, StoredValue::AddressableEntity(new_entity));

        if let Some(named_keys) = maybe_named_keys {
            let entity_addr = EntityAddr::new_system_entity_addr(contract_hash.value());

            for (string, key) in named_keys.into_inner().into_iter() {
                let entry_addr = NamedKeyAddr::new_from_string(entity_addr, string.clone())
                    .map_err(ProtocolUpgradeError::Bytesrepr)?;

                let named_key_value = NamedKeyValue::from_concrete_values(key, string)
                    .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

                let entry_key = Key::NamedKey(entry_addr);

                self.tracking_copy
                    .borrow_mut()
                    .write(entry_key, StoredValue::NamedKey(named_key_value));
            }
        }

        package.insert_entity_version(self.new_protocol_version.value().major, contract_hash);

        self.tracking_copy.borrow_mut().write(
            Key::Package(contract.package_hash().value()),
            StoredValue::Package(package),
        );

        if must_prune {
            // Start pruning legacy records
            self.tracking_copy
                .borrow_mut()
                .prune(Key::Hash(contract.package_hash().value()));
            self.tracking_copy
                .borrow_mut()
                .prune(Key::Hash(contract_hash.value()));
            let contract_wasm_key = Key::Hash(contract.byte_code_hash().value());

            self.tracking_copy.borrow_mut().prune(contract_wasm_key);
        }

        Ok(())
    }

    fn retrieve_system_package(
        &self,
        package_hash: PackageHash,
        system_contract_type: SystemEntityType,
    ) -> Result<Package, ProtocolUpgradeError> {
        if let Some(StoredValue::Package(system_entity)) = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::Package(package_hash.value()))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                    system_contract_type.to_string(),
                )
            })?
        {
            return Ok(system_entity);
        }

        if let Some(StoredValue::ContractPackage(contract_package)) = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::Hash(package_hash.value()))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                    system_contract_type.to_string(),
                )
            })?
        {
            let package: Package = contract_package.into();

            return Ok(package);
        }

        Err(ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
            system_contract_type.to_string(),
        ))
    }

    fn retrieve_system_contract(
        &self,
        contract_hash: AddressableEntityHash,
        system_contract_type: SystemEntityType,
    ) -> Result<(AddressableEntity, Option<NamedKeys>, bool), ProtocolUpgradeError> {
        if let Some(StoredValue::AddressableEntity(system_entity)) = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::AddressableEntity(EntityAddr::new_system_entity_addr(
                contract_hash.value(),
            )))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(
                    system_contract_type.to_string(),
                )
            })?
        {
            return Ok((system_entity, None, NO_PRUNE));
        }

        if let Some(StoredValue::Contract(system_contract)) = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::Hash(contract_hash.value()))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(
                    system_contract_type.to_string(),
                )
            })?
        {
            let named_keys = system_contract.named_keys().clone();

            return Ok((system_contract.into(), Some(named_keys), PRUNE));
        }

        Err(ProtocolUpgradeError::UnableToRetrieveSystemContract(
            system_contract_type.to_string(),
        ))
    }

    pub(crate) fn migrate_system_account(
        &self,
        pre_state_hash: Digest,
    ) -> Result<(), ProtocolUpgradeError> {
        let mut address_generator = AddressGenerator::new(pre_state_hash.as_ref(), Phase::System);

        let byte_code_hash = ByteCodeHash::default();
        let entity_hash = AddressableEntityHash::new(address_generator.new_hash_address());
        let package_hash = PackageHash::new(address_generator.new_hash_address());

        let byte_code = ByteCode::new(ByteCodeKind::Empty, vec![]);

        let account_hash = PublicKey::System.to_account_hash();
        let associated_keys = AssociatedKeys::new(account_hash, Weight::new(1));

        let main_purse = {
            let purse_addr = address_generator.new_hash_address();
            let balance_cl_value = CLValue::from_t(U512::zero())
                .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

            self.tracking_copy.borrow_mut().write(
                Key::Balance(purse_addr),
                StoredValue::CLValue(balance_cl_value),
            );

            let purse_cl_value = CLValue::unit();
            let purse_uref = URef::new(purse_addr, AccessRights::READ_ADD_WRITE);

            self.tracking_copy
                .borrow_mut()
                .write(Key::URef(purse_uref), StoredValue::CLValue(purse_cl_value));
            purse_uref
        };

        let system_account_entity = AddressableEntity::new(
            package_hash,
            byte_code_hash,
            EntryPoints::new(),
            self.new_protocol_version,
            main_purse,
            associated_keys,
            ActionThresholds::default(),
            MessageTopics::default(),
            EntityKind::Account(account_hash),
        );

        let access_key = address_generator.new_uref(AccessRights::READ_ADD_WRITE);

        let package = {
            let mut package = Package::new(
                access_key,
                EntityVersions::default(),
                BTreeSet::default(),
                Groups::default(),
                PackageStatus::default(),
            );
            package.insert_entity_version(self.new_protocol_version.value().major, entity_hash);
            package
        };

        let byte_code_key = Key::ByteCode(ByteCodeAddr::Empty);
        self.tracking_copy
            .borrow_mut()
            .write(byte_code_key, StoredValue::ByteCode(byte_code));

        let entity_key = system_account_entity.entity_key(entity_hash);

        self.tracking_copy.borrow_mut().write(
            entity_key,
            StoredValue::AddressableEntity(system_account_entity),
        );

        self.tracking_copy
            .borrow_mut()
            .write(package_hash.into(), StoredValue::Package(package));

        let contract_by_account = CLValue::from_t(entity_key)
            .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

        self.tracking_copy.borrow_mut().write(
            Key::Account(account_hash),
            StoredValue::CLValue(contract_by_account),
        );

        Ok(())
    }

    /// Creates an accumulation purse in the handle payment system contract if its not present.
    ///
    /// This can happen on older networks that did not have support for [`FeeHandling::Accumulate`]
    /// at the genesis. In such cases we have to check the state of handle payment contract and
    /// create an accumulation purse.
    pub(crate) fn create_accumulation_purse_if_required(
        &self,
        handle_payment_hash: &AddressableEntityHash,
        engine_config: &EngineConfig,
    ) -> Result<(), ProtocolUpgradeError> {
        match engine_config.fee_handling() {
            FeeHandling::PayToProposer | FeeHandling::Burn => return Ok(()),
            FeeHandling::Accumulate => {}
        }
        let mut address_generator = {
            let seed_bytes = (self.old_protocol_version, self.new_protocol_version).to_bytes()?;
            let phase = Phase::System;
            AddressGenerator::new(&seed_bytes, phase)
        };
        let system_contract = SystemEntityType::HandlePayment;

        let (addressable_entity, maybe_named_keys, _) =
            self.retrieve_system_contract(*handle_payment_hash, system_contract)?;

        let entity_addr = EntityAddr::new_system_entity_addr(handle_payment_hash.value());

        if let Some(named_keys) = maybe_named_keys {
            for (string, key) in named_keys.into_inner().into_iter() {
                let entry_addr = NamedKeyAddr::new_from_string(entity_addr, string.clone())
                    .map_err(ProtocolUpgradeError::Bytesrepr)?;

                let named_key_value = NamedKeyValue::from_concrete_values(key, string)
                    .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

                let entry_key = Key::NamedKey(entry_addr);

                self.tracking_copy
                    .borrow_mut()
                    .write(entry_key, StoredValue::NamedKey(named_key_value));
            }
        }

        let named_key_addr =
            NamedKeyAddr::new_from_string(entity_addr, ACCUMULATION_PURSE_KEY.to_string())
                .map_err(ProtocolUpgradeError::Bytesrepr)?;

        let requries_accumulation_purse = self
            .tracking_copy
            .borrow_mut()
            .read(&Key::NamedKey(named_key_addr))
            .map_err(|_| ProtocolUpgradeError::UnexpectedStoredValueVariant)?
            .is_none();

        if requries_accumulation_purse {
            let purse_uref = address_generator.new_uref(AccessRights::READ_ADD_WRITE);
            let balance_clvalue = CLValue::from_t(U512::zero())?;
            self.tracking_copy.borrow_mut().write(
                Key::Balance(purse_uref.addr()),
                StoredValue::CLValue(balance_clvalue),
            );

            let purse_key = Key::URef(purse_uref);

            self.tracking_copy
                .borrow_mut()
                .write(purse_key, StoredValue::CLValue(CLValue::unit()));

            let purse =
                NamedKeyValue::from_concrete_values(purse_key, ACCUMULATION_PURSE_KEY.to_string())
                    .map_err(|cl_error| ProtocolUpgradeError::CLValue(cl_error.to_string()))?;

            self.tracking_copy
                .borrow_mut()
                .write(Key::NamedKey(named_key_addr), StoredValue::NamedKey(purse));

            let entity_key =
                Key::addressable_entity_key(EntityKindTag::System, *handle_payment_hash);

            self.tracking_copy.borrow_mut().write(
                entity_key,
                StoredValue::AddressableEntity(addressable_entity),
            );
        }

        Ok(())
    }
}
