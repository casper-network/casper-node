//! Support for applying upgrades on the execution engine.
use std::{cell::RefCell, collections::BTreeSet, fmt, rc::Rc};

use thiserror::Error;

use casper_storage::global_state::state::StateProvider;
use casper_types::{
    addressable_entity::{ActionThresholds, AssociatedKeys, NamedKeys, Weight},
    bytesrepr::{self, ToBytes},
    execution::Effects,
    package::{ContractPackageKind, ContractPackageStatus, ContractVersions, Groups},
    system::{handle_payment::ACCUMULATION_PURSE_KEY, SystemContractType},
    AccessRights, AddressableEntity, AddressableEntityHash, CLValue, CLValueError,
    ContractPackageHash, ContractWasm, Digest, EntryPoints, FeeHandling, Key, Package, Phase,
    ProtocolVersion, PublicKey, StoredValue, URef, U512,
};

use crate::{
    engine_state::ACCOUNT_WASM_HASH, execution::AddressGenerator, tracking_copy::TrackingCopy,
};

use super::EngineConfig;

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
        standard_payment_hash: &AddressableEntityHash,
    ) -> Result<(), ProtocolUpgradeError> {
        self.refresh_system_contract_entry_points(*mint_hash, SystemContractType::Mint)?;
        self.refresh_system_contract_entry_points(*auction_hash, SystemContractType::Auction)?;
        self.refresh_system_contract_entry_points(
            *handle_payment_hash,
            SystemContractType::HandlePayment,
        )?;
        self.refresh_system_contract_entry_points(
            *standard_payment_hash,
            SystemContractType::StandardPayment,
        )?;

        Ok(())
    }

    /// Refresh the system contracts with an updated set of entry points,
    /// and bump the contract version at a major version upgrade.
    fn refresh_system_contract_entry_points(
        &self,
        contract_hash: AddressableEntityHash,
        system_contract_type: SystemContractType,
    ) -> Result<(), ProtocolUpgradeError> {
        let contract_name = system_contract_type.contract_name();
        let entry_points = system_contract_type.contract_entry_points();

        let mut contract = self.retrieve_system_contract(contract_hash, system_contract_type)?;

        let contract_package_key = Key::Hash(contract.contract_package_hash().value());

        let mut contract_package = if let StoredValue::ContractPackage(contract_package) = self
            .tracking_copy
            .borrow_mut()
            .read(&contract_package_key)
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                    contract_name.to_string(),
                )
            })?
            .ok_or_else(|| {
                ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                    contract_name.to_string(),
                )
            })? {
            contract_package
        } else {
            return Err(ProtocolUpgradeError::UnableToRetrieveSystemContractPackage(
                contract_name,
            ));
        };

        // Update the package kind from legacy to system contract
        if contract_package.is_legacy() {
            contract_package.update_package_kind(ContractPackageKind::System(system_contract_type))
        }

        contract_package
            .disable_contract_version(contract_hash)
            .map_err(|_| {
                ProtocolUpgradeError::FailedToDisablePreviousVersion(contract_name.to_string())
            })?;

        contract.set_protocol_version(self.new_protocol_version);

        let new_entity = AddressableEntity::new(
            contract.contract_package_hash(),
            contract.contract_wasm_hash(),
            contract.named_keys().clone(),
            entry_points,
            self.new_protocol_version,
            URef::default(),
            AssociatedKeys::default(),
            ActionThresholds::default(),
        );
        self.tracking_copy.borrow_mut().write(
            contract_hash.into(),
            StoredValue::AddressableEntity(new_entity),
        );

        contract_package
            .insert_contract_version(self.new_protocol_version.value().major, contract_hash);

        self.tracking_copy.borrow_mut().write(
            contract_package_key,
            StoredValue::ContractPackage(contract_package),
        );

        Ok(())
    }

    fn retrieve_system_contract(
        &self,
        contract_hash: AddressableEntityHash,
        system_contract_type: SystemContractType,
    ) -> Result<AddressableEntity, ProtocolUpgradeError> {
        match self
            .tracking_copy
            .borrow_mut()
            .read(&contract_hash.into())
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(
                    system_contract_type.to_string(),
                )
            })? {
            None => Err(ProtocolUpgradeError::UnableToRetrieveSystemContract(
                system_contract_type.to_string(),
            )),
            Some(StoredValue::AddressableEntity(entity)) => Ok(entity),
            Some(StoredValue::Contract(contract)) => Ok(contract.into()),
            Some(_) => Err(ProtocolUpgradeError::UnableToRetrieveSystemContract(
                system_contract_type.to_string(),
            )),
        }
    }

    pub(crate) fn migrate_system_account(
        &self,
        pre_state_hash: Digest,
    ) -> Result<(), ProtocolUpgradeError> {
        let mut address_generator = AddressGenerator::new(pre_state_hash.as_ref(), Phase::System);

        let contract_wasm_hash = *ACCOUNT_WASM_HASH;
        let contract_hash = AddressableEntityHash::new(address_generator.new_hash_address());
        let contract_package_hash = ContractPackageHash::new(address_generator.new_hash_address());

        let contract_wasm = ContractWasm::new(vec![]);

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

        let contract = AddressableEntity::new(
            contract_package_hash,
            contract_wasm_hash,
            NamedKeys::new(),
            EntryPoints::new(),
            self.new_protocol_version,
            main_purse,
            associated_keys,
            ActionThresholds::default(),
        );

        let access_key = address_generator.new_uref(AccessRights::READ_ADD_WRITE);

        let contract_package = {
            let mut contract_package = Package::new(
                access_key,
                ContractVersions::default(),
                BTreeSet::default(),
                Groups::default(),
                ContractPackageStatus::default(),
                ContractPackageKind::Account(account_hash),
            );
            contract_package
                .insert_contract_version(self.new_protocol_version.value().major, contract_hash);
            contract_package
        };

        self.tracking_copy.borrow_mut().write(
            contract_wasm_hash.into(),
            StoredValue::ContractWasm(contract_wasm),
        );
        self.tracking_copy.borrow_mut().write(
            contract_hash.into(),
            StoredValue::AddressableEntity(contract),
        );
        self.tracking_copy.borrow_mut().write(
            contract_package_hash.into(),
            StoredValue::ContractPackage(contract_package),
        );

        let contract_key: Key = contract_hash.into();
        let contract_by_account = CLValue::from_t(contract_key)
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
        let system_contract = SystemContractType::HandlePayment;

        let mut addressable_entity =
            self.retrieve_system_contract(*handle_payment_hash, system_contract)?;
        if !addressable_entity
            .named_keys()
            .contains(ACCUMULATION_PURSE_KEY)
        {
            let purse_uref = address_generator.new_uref(AccessRights::READ_ADD_WRITE);
            let balance_clvalue = CLValue::from_t(U512::zero())?;
            self.tracking_copy.borrow_mut().write(
                Key::Balance(purse_uref.addr()),
                StoredValue::CLValue(balance_clvalue),
            );
            self.tracking_copy
                .borrow_mut()
                .write(Key::URef(purse_uref), StoredValue::CLValue(CLValue::unit()));

            let mut new_named_keys = NamedKeys::new();
            new_named_keys.insert(ACCUMULATION_PURSE_KEY.into(), Key::from(purse_uref));
            addressable_entity.named_keys_append(new_named_keys);
            self.tracking_copy.borrow_mut().write(
                (*handle_payment_hash).into(),
                StoredValue::AddressableEntity(addressable_entity),
            );
        }

        Ok(())
    }
}
