//! Support for applying upgrades on the execution engine.
use num_rational::Ratio;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use thiserror::Error;
use tracing::{debug, error, info, warn};

use casper_types::{
    addressable_entity::{
        ActionThresholds, AssociatedKeys, EntityKind, NamedKeyAddr, NamedKeyValue, Weight,
    },
    bytesrepr::{self, ToBytes},
    contracts::{ContractHash, NamedKeys},
    system::{
        auction::{
            BidAddr, BidKind, DelegatorBid, DelegatorKind, SeigniorageRecipientsSnapshotV1,
            SeigniorageRecipientsSnapshotV2, SeigniorageRecipientsV2, Unbond, ValidatorBid,
            AUCTION_DELAY_KEY, DEFAULT_SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION,
            LOCKED_FUNDS_PERIOD_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY, UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        handle_payment::{ACCUMULATION_PURSE_KEY, PAYMENT_PURSE_KEY},
        mint::{
            MINT_GAS_HOLD_HANDLING_KEY, MINT_GAS_HOLD_INTERVAL_KEY, ROUND_SEIGNIORAGE_RATE_KEY,
            TOTAL_SUPPLY_KEY,
        },
        SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT,
    },
    AccessRights, AddressableEntity, AddressableEntityHash, ByteCode, ByteCodeAddr, ByteCodeHash,
    ByteCodeKind, CLValue, CLValueError, Contract, Digest, EntityAddr, EntityVersions,
    EntryPointAddr, EntryPointValue, EntryPoints, FeeHandling, Groups, HashAddr, Key, KeyTag,
    Motes, Package, PackageHash, PackageStatus, Phase, ProtocolUpgradeConfig, ProtocolVersion,
    PublicKey, StoredValue, SystemHashRegistry, URef, U512,
};

use crate::{
    global_state::state::StateProvider,
    tracking_copy::{TrackingCopy, TrackingCopyEntityExt, TrackingCopyExt},
    AddressGenerator,
};

const NO_CARRY_FORWARD: bool = false;
const CARRY_FORWARD: bool = true;

/// Represents outcomes of a failed protocol upgrade.
#[derive(Clone, Error, Debug)]
pub enum ProtocolUpgradeError {
    /// Protocol version used in the deploy is invalid.
    #[error("Invalid protocol version: {0}")]
    InvalidProtocolVersion(ProtocolVersion),
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
    #[error("Bytesrepr error: {0}")]
    Bytesrepr(String),
    /// Failed to create system entity registry.
    #[error("Failed to insert system entity registry")]
    FailedToCreateSystemRegistry,
    /// Found unexpected variant of a key.
    #[error("Unexpected key variant")]
    UnexpectedKeyVariant,
    /// Found unexpected variant of a stored value.
    #[error("Unexpected stored value variant")]
    UnexpectedStoredValueVariant,
    /// Failed to convert into a CLValue.
    #[error("{0}")]
    CLValue(String),
    /// Missing system contract hash.
    #[error("Missing system contract hash: {0}")]
    MissingSystemEntityHash(String),
    /// Tracking copy error.
    #[error("{0}")]
    TrackingCopy(crate::tracking_copy::TrackingCopyError),
}

impl From<CLValueError> for ProtocolUpgradeError {
    fn from(v: CLValueError) -> Self {
        Self::CLValue(v.to_string())
    }
}

impl From<crate::tracking_copy::TrackingCopyError> for ProtocolUpgradeError {
    fn from(err: crate::tracking_copy::TrackingCopyError) -> Self {
        ProtocolUpgradeError::TrackingCopy(err)
    }
}

impl From<bytesrepr::Error> for ProtocolUpgradeError {
    fn from(error: bytesrepr::Error) -> Self {
        ProtocolUpgradeError::Bytesrepr(error.to_string())
    }
}

/// Addresses for system entities.
pub struct SystemHashAddresses {
    mint: HashAddr,
    auction: HashAddr,
    handle_payment: HashAddr,
}

impl SystemHashAddresses {
    /// Creates a new instance of system entity addresses.
    pub fn new(mint: HashAddr, auction: HashAddr, handle_payment: HashAddr) -> Self {
        SystemHashAddresses {
            mint,
            auction,
            handle_payment,
        }
    }

    /// Mint address.
    pub fn mint(&self) -> HashAddr {
        self.mint
    }

    /// Auction address.
    pub fn auction(&self) -> HashAddr {
        self.auction
    }

    /// Handle payment address.
    pub fn handle_payment(&self) -> HashAddr {
        self.handle_payment
    }
}

/// The system upgrader deals with conducting an actual protocol upgrade.
pub struct ProtocolUpgrader<S>
where
    S: StateProvider + ?Sized,
{
    config: ProtocolUpgradeConfig,
    address_generator: Rc<RefCell<AddressGenerator>>,
    tracking_copy: TrackingCopy<<S as StateProvider>::Reader>,
}

impl<S> ProtocolUpgrader<S>
where
    S: StateProvider + ?Sized,
{
    /// Creates new system upgrader instance.
    pub fn new(
        config: ProtocolUpgradeConfig,
        protocol_upgrade_config_hash: Digest,
        tracking_copy: TrackingCopy<<S as StateProvider>::Reader>,
    ) -> Self {
        let phase = Phase::System;
        let protocol_upgrade_config_hash_bytes = protocol_upgrade_config_hash.as_ref();

        let address_generator = {
            let generator = AddressGenerator::new(protocol_upgrade_config_hash_bytes, phase);
            Rc::new(RefCell::new(generator))
        };
        ProtocolUpgrader {
            config,
            address_generator,
            tracking_copy,
        }
    }

    /// Apply a protocol upgrade.
    pub fn upgrade(
        mut self,
        pre_state_hash: Digest,
    ) -> Result<TrackingCopy<<S as StateProvider>::Reader>, ProtocolUpgradeError> {
        self.check_next_protocol_version_validity()?;
        self.handle_global_state_updates();
        let system_entity_addresses = self.handle_system_hashes()?;

        if self.config.enable_addressable_entity() {
            self.migrate_system_account(pre_state_hash)?;
            self.create_accumulation_purse_if_required(
                &system_entity_addresses.handle_payment(),
                self.config.fee_handling(),
            )?;
            self.migrate_or_refresh_system_entities(&system_entity_addresses)?;
        } else {
            self.create_accumulation_purse_if_required_by_contract(
                &system_entity_addresses.handle_payment(),
                self.config.fee_handling(),
            )?;
            self.refresh_system_contracts(&system_entity_addresses)?;
        }

        self.handle_payment_purse_check(
            system_entity_addresses.handle_payment(),
            system_entity_addresses.mint(),
        )?;
        self.handle_new_gas_hold_config(system_entity_addresses.mint())?;
        self.handle_new_validator_slots(system_entity_addresses.auction())?;
        self.handle_new_auction_delay(system_entity_addresses.auction())?;
        self.handle_new_locked_funds_period_millis(system_entity_addresses.auction())?;
        self.handle_new_unbonding_delay(system_entity_addresses.auction())?;
        self.handle_new_round_seigniorage_rate(system_entity_addresses.mint())?;
        self.handle_unbonds_migration()?;
        self.handle_bids_migration(
            self.config.minimum_delegation_amount(),
            self.config.maximum_delegation_amount(),
        )?;
        self.handle_era_info_migration()?;
        self.handle_seignorage_snapshot_migration(system_entity_addresses.auction())?;

        Ok(self.tracking_copy)
    }

    /// Determine if the next protocol version is a legitimate semver progression.
    pub fn check_next_protocol_version_validity(&self) -> Result<(), ProtocolUpgradeError> {
        debug!("check next protocol version validity");
        let current_protocol_version = self.config.current_protocol_version();
        let new_protocol_version = self.config.new_protocol_version();

        let upgrade_check_result =
            current_protocol_version.check_next_version(&new_protocol_version);

        if upgrade_check_result.is_invalid() {
            Err(ProtocolUpgradeError::InvalidProtocolVersion(
                new_protocol_version,
            ))
        } else {
            Ok(())
        }
    }

    fn system_hash_registry(&self) -> Result<SystemHashRegistry, ProtocolUpgradeError> {
        debug!("system entity registry");
        let registry = if let Ok(registry) = self.tracking_copy.get_system_entity_registry() {
            registry
        } else {
            // Check the upgrade config for the registry
            let upgrade_registry = self
                .config
                .global_state_update()
                .get(&Key::SystemEntityRegistry)
                .ok_or_else(|| {
                    error!("Registry is absent in upgrade config");
                    ProtocolUpgradeError::FailedToCreateSystemRegistry
                })?
                .to_owned();
            if let StoredValue::CLValue(cl_registry) = upgrade_registry {
                CLValue::into_t::<SystemHashRegistry>(cl_registry).map_err(|error| {
                    let error_msg = format!("Conversion to system registry failed: {:?}", error);
                    error!("{}", error_msg);
                    ProtocolUpgradeError::Bytesrepr(error_msg)
                })?
            } else {
                error!("Failed to create registry as StoreValue in upgrade config is not CLValue");
                return Err(ProtocolUpgradeError::FailedToCreateSystemRegistry);
            }
        };
        Ok(registry)
    }

    /// Handle system entities.
    pub fn handle_system_hashes(&mut self) -> Result<SystemHashAddresses, ProtocolUpgradeError> {
        debug!("handle system entities");
        let mut registry = self.system_hash_registry()?;

        let mint = *registry.get(MINT).ok_or_else(|| {
            error!("Missing system mint entity hash");
            ProtocolUpgradeError::MissingSystemEntityHash(MINT.to_string())
        })?;
        let auction = *registry.get(AUCTION).ok_or_else(|| {
            error!("Missing system auction entity hash");
            ProtocolUpgradeError::MissingSystemEntityHash(AUCTION.to_string())
        })?;
        let handle_payment = *registry.get(HANDLE_PAYMENT).ok_or_else(|| {
            error!("Missing system handle payment entity hash");
            ProtocolUpgradeError::MissingSystemEntityHash(HANDLE_PAYMENT.to_string())
        })?;
        if let Some(standard_payment_hash) = registry.remove_standard_payment() {
            // Write the chainspec registry to global state
            let cl_value_chainspec_registry = CLValue::from_t(registry)
                .map_err(|error| ProtocolUpgradeError::Bytesrepr(error.to_string()))?;

            self.tracking_copy.write(
                Key::SystemEntityRegistry,
                StoredValue::CLValue(cl_value_chainspec_registry),
            );

            // Prune away standard payment from global state.
            self.tracking_copy.prune(Key::Hash(standard_payment_hash));
        };

        // Write the chainspec registry to global state
        let cl_value_chainspec_registry = CLValue::from_t(self.config.chainspec_registry().clone())
            .map_err(|error| ProtocolUpgradeError::Bytesrepr(error.to_string()))?;

        self.tracking_copy.write(
            Key::ChainspecRegistry,
            StoredValue::CLValue(cl_value_chainspec_registry),
        );

        let system_hash_addresses = SystemHashAddresses::new(mint, auction, handle_payment);

        Ok(system_hash_addresses)
    }

    /// Bump major version and/or update the entry points for system contracts.
    pub fn migrate_or_refresh_system_entities(
        &mut self,
        system_entity_addresses: &SystemHashAddresses,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!("refresh system contracts");
        self.migrate_or_refresh_system_entity_entry_points(
            system_entity_addresses.mint(),
            SystemEntityType::Mint,
        )?;
        self.migrate_or_refresh_system_entity_entry_points(
            system_entity_addresses.auction(),
            SystemEntityType::Auction,
        )?;
        self.migrate_or_refresh_system_entity_entry_points(
            system_entity_addresses.handle_payment(),
            SystemEntityType::HandlePayment,
        )?;

        Ok(())
    }

    /// Bump major version and/or update the entry points for system contracts.
    pub fn refresh_system_contracts(
        &mut self,
        system_entity_addresses: &SystemHashAddresses,
    ) -> Result<(), ProtocolUpgradeError> {
        self.refresh_system_contract_entry_points(
            system_entity_addresses.mint(),
            SystemEntityType::Mint,
        )?;
        self.refresh_system_contract_entry_points(
            system_entity_addresses.auction(),
            SystemEntityType::Auction,
        )?;
        self.refresh_system_contract_entry_points(
            system_entity_addresses.handle_payment(),
            SystemEntityType::HandlePayment,
        )?;

        Ok(())
    }

    /// Refresh the system contracts with an updated set of entry points,
    /// and bump the contract version at a major version upgrade.
    fn migrate_or_refresh_system_entity_entry_points(
        &mut self,
        hash_addr: HashAddr,
        system_entity_type: SystemEntityType,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!(%system_entity_type, "refresh system contract entry points");
        let entity_name = system_entity_type.entity_name();

        let (mut entity, maybe_named_keys, must_carry_forward) =
            match self.retrieve_system_entity(hash_addr, system_entity_type) {
                Ok(ret) => ret,
                Err(err) => {
                    error!("{:?}", err);
                    return Err(err);
                }
            };

        let mut package =
            self.retrieve_system_package(entity.package_hash(), system_entity_type)?;

        let entity_hash = AddressableEntityHash::new(hash_addr);
        package.disable_entity_version(entity_hash).map_err(|_| {
            ProtocolUpgradeError::FailedToDisablePreviousVersion(entity_name.to_string())
        })?;

        entity.set_protocol_version(self.config.new_protocol_version());

        let new_entity = AddressableEntity::new(
            entity.package_hash(),
            ByteCodeHash::default(),
            self.config.new_protocol_version(),
            URef::default(),
            AssociatedKeys::default(),
            ActionThresholds::default(),
            EntityKind::System(system_entity_type),
        );

        let byte_code_key = Key::byte_code_key(ByteCodeAddr::Empty);
        let byte_code = ByteCode::new(ByteCodeKind::Empty, vec![]);

        self.tracking_copy
            .write(byte_code_key, StoredValue::ByteCode(byte_code));

        let entity_key = new_entity.entity_key(entity_hash);

        self.tracking_copy
            .write(entity_key, StoredValue::AddressableEntity(new_entity));

        let entity_addr = EntityAddr::new_system(entity_hash.value());

        if let Some(named_keys) = maybe_named_keys {
            for (string, key) in named_keys.into_inner().into_iter() {
                let entry_addr = NamedKeyAddr::new_from_string(entity_addr, string.clone())
                    .map_err(|err| ProtocolUpgradeError::Bytesrepr(err.to_string()))?;

                let entry_key = Key::NamedKey(entry_addr);

                let named_key_value = NamedKeyValue::from_concrete_values(key, string)
                    .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

                self.tracking_copy
                    .write(entry_key, StoredValue::NamedKey(named_key_value));
            }
        }

        let entry_points = system_entity_type.entry_points();

        for entry_point in entry_points.take_entry_points() {
            let entry_point_addr =
                EntryPointAddr::new_v1_entry_point_addr(entity_addr, entry_point.name())
                    .map_err(|error| ProtocolUpgradeError::Bytesrepr(error.to_string()))?;
            self.tracking_copy.write(
                Key::EntryPoint(entry_point_addr),
                StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point)),
            );
        }

        package.insert_entity_version(
            self.config.new_protocol_version().value().major,
            entity_hash,
        );

        self.tracking_copy.write(
            Key::SmartContract(entity.package_hash().value()),
            StoredValue::SmartContract(package),
        );

        if must_carry_forward {
            // carry forward
            let package_key = Key::SmartContract(entity.package_hash().value());
            let uref = URef::default();
            let indirection = CLValue::from_t((package_key, uref))
                .map_err(|cl_error| ProtocolUpgradeError::CLValue(cl_error.to_string()))?;

            self.tracking_copy.write(
                Key::Hash(entity.package_hash().value()),
                StoredValue::CLValue(indirection),
            );

            let contract_wasm_key = Key::Hash(entity.byte_code_hash().value());
            let contract_wasm_indirection = CLValue::from_t(Key::ByteCode(ByteCodeAddr::Empty))
                .map_err(|cl_error| ProtocolUpgradeError::CLValue(cl_error.to_string()))?;
            self.tracking_copy.write(
                contract_wasm_key,
                StoredValue::CLValue(contract_wasm_indirection),
            );

            let contract_indirection = CLValue::from_t(Key::AddressableEntity(entity_addr))
                .map_err(|cl_error| ProtocolUpgradeError::CLValue(cl_error.to_string()))?;

            self.tracking_copy.write(
                Key::Hash(entity_addr.value()),
                StoredValue::CLValue(contract_indirection),
            )
        }

        Ok(())
    }

    fn retrieve_system_package(
        &mut self,
        package_hash: PackageHash,
        system_contract_type: SystemEntityType,
    ) -> Result<Package, ProtocolUpgradeError> {
        debug!(%system_contract_type, "retrieve system package");
        if let Some(StoredValue::SmartContract(system_entity)) = self
            .tracking_copy
            .read(&Key::SmartContract(package_hash.value()))
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

    fn retrieve_system_entity(
        &mut self,
        hash_addr: HashAddr,
        system_contract_type: SystemEntityType,
    ) -> Result<(AddressableEntity, Option<NamedKeys>, bool), ProtocolUpgradeError> {
        debug!(%system_contract_type, "retrieve system entity");
        if let Some(StoredValue::Contract(system_contract)) = self
            .tracking_copy
            .read(&Key::Hash(hash_addr))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(
                    system_contract_type.to_string(),
                )
            })?
        {
            let named_keys = system_contract.named_keys().clone();
            return Ok((system_contract.into(), Some(named_keys), CARRY_FORWARD));
        }

        if let Some(StoredValue::AddressableEntity(system_entity)) = self
            .tracking_copy
            .read(&Key::AddressableEntity(EntityAddr::new_system(hash_addr)))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(
                    system_contract_type.to_string(),
                )
            })?
        {
            return Ok((system_entity, None, NO_CARRY_FORWARD));
        }

        Err(ProtocolUpgradeError::UnableToRetrieveSystemContract(
            system_contract_type.to_string(),
        ))
    }

    /// Refresh the system contracts with an updated set of entry points,
    /// and bump the contract version at a major version upgrade.
    fn refresh_system_contract_entry_points(
        &mut self,
        contract_hash: HashAddr,
        system_entity_type: SystemEntityType,
    ) -> Result<(), ProtocolUpgradeError> {
        let contract_name = system_entity_type.entity_name();
        let entry_points = system_entity_type.entry_points();

        let mut contract = if let StoredValue::Contract(contract) = self
            .tracking_copy
            .read(&Key::Hash(contract_hash))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(contract_name.to_string())
            })?
            .ok_or_else(|| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(contract_name.to_string())
            })? {
            contract
        } else {
            return Err(ProtocolUpgradeError::UnableToRetrieveSystemContract(
                contract_name,
            ));
        };

        let is_major_bump = self
            .config
            .current_protocol_version()
            .check_next_version(&self.config.new_protocol_version())
            .is_major_version();

        let contract_entry_points: EntryPoints = contract.entry_points().clone().into();
        let entry_points_unchanged = contract_entry_points == entry_points;
        if entry_points_unchanged && !is_major_bump {
            // We don't need to do anything if entry points are unchanged, or there's no major
            // version bump.
            return Ok(());
        }

        let contract_package_key = Key::Hash(contract.contract_package_hash().value());

        let mut contract_package = if let StoredValue::ContractPackage(contract_package) = self
            .tracking_copy
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

        contract.set_protocol_version(self.config.new_protocol_version());

        let new_contract = Contract::new(
            contract.contract_package_hash(),
            contract.contract_wasm_hash(),
            contract.named_keys().clone(),
            entry_points.into(),
            self.config.new_protocol_version(),
        );
        self.tracking_copy.write(
            Key::Hash(contract_hash),
            StoredValue::Contract(new_contract),
        );

        contract_package.insert_contract_version(
            self.config.new_protocol_version().value().major,
            ContractHash::new(contract_hash),
        );

        self.tracking_copy.write(
            contract_package_key,
            StoredValue::ContractPackage(contract_package),
        );

        Ok(())
    }

    /// Migrate the system account to addressable entity if necessary.
    pub fn migrate_system_account(
        &mut self,
        pre_state_hash: Digest,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!("migrate system account");
        let mut address_generator = AddressGenerator::new(pre_state_hash.as_ref(), Phase::System);

        let account_hash = PublicKey::System.to_account_hash();

        let main_purse = {
            let purse_addr = address_generator.new_hash_address();
            let balance_cl_value = CLValue::from_t(U512::zero())
                .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

            self.tracking_copy.write(
                Key::Balance(purse_addr),
                StoredValue::CLValue(balance_cl_value),
            );

            let purse_cl_value = CLValue::unit();
            let purse_uref = URef::new(purse_addr, AccessRights::READ_ADD_WRITE);

            self.tracking_copy
                .write(Key::URef(purse_uref), StoredValue::CLValue(purse_cl_value));
            purse_uref
        };

        let associated_keys = AssociatedKeys::new(account_hash, Weight::new(1));
        let byte_code_hash = ByteCodeHash::default();
        let entity_hash = AddressableEntityHash::new(PublicKey::System.to_account_hash().value());
        let package_hash = PackageHash::new(address_generator.new_hash_address());

        let byte_code = ByteCode::new(ByteCodeKind::Empty, vec![]);

        let system_account_entity = AddressableEntity::new(
            package_hash,
            byte_code_hash,
            self.config.new_protocol_version(),
            main_purse,
            associated_keys,
            ActionThresholds::default(),
            EntityKind::Account(account_hash),
        );

        let package = {
            let mut package = Package::new(
                EntityVersions::default(),
                BTreeSet::default(),
                Groups::default(),
                PackageStatus::default(),
            );
            package.insert_entity_version(
                self.config.new_protocol_version().value().major,
                entity_hash,
            );
            package
        };

        let byte_code_key = Key::ByteCode(ByteCodeAddr::Empty);
        self.tracking_copy
            .write(byte_code_key, StoredValue::ByteCode(byte_code));

        let entity_key = system_account_entity.entity_key(entity_hash);

        self.tracking_copy.write(
            entity_key,
            StoredValue::AddressableEntity(system_account_entity),
        );

        self.tracking_copy
            .write(package_hash.into(), StoredValue::SmartContract(package));

        let contract_by_account = CLValue::from_t(entity_key)
            .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

        self.tracking_copy.write(
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
    pub fn create_accumulation_purse_if_required(
        &mut self,
        handle_payment_hash: &HashAddr,
        fee_handling: FeeHandling,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!(?fee_handling, "create accumulation purse if required");
        match fee_handling {
            FeeHandling::PayToProposer | FeeHandling::Burn => return Ok(()),
            FeeHandling::Accumulate | FeeHandling::NoFee => {}
        }
        let mut address_generator = {
            let seed_bytes = (
                self.config.current_protocol_version(),
                self.config.new_protocol_version(),
            )
                .to_bytes()?;
            let phase = Phase::System;
            AddressGenerator::new(&seed_bytes, phase)
        };
        let system_contract = SystemEntityType::HandlePayment;

        let (addressable_entity, maybe_named_keys, _) =
            self.retrieve_system_entity(*handle_payment_hash, system_contract)?;

        let entity_addr = EntityAddr::new_system(*handle_payment_hash);

        if let Some(named_keys) = maybe_named_keys {
            for (string, key) in named_keys.into_inner().into_iter() {
                let entry_addr = NamedKeyAddr::new_from_string(entity_addr, string.clone())
                    .map_err(|err| ProtocolUpgradeError::Bytesrepr(err.to_string()))?;

                let named_key_value = NamedKeyValue::from_concrete_values(key, string)
                    .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

                let entry_key = Key::NamedKey(entry_addr);

                self.tracking_copy
                    .write(entry_key, StoredValue::NamedKey(named_key_value));
            }
        }

        let named_key_addr =
            NamedKeyAddr::new_from_string(entity_addr, ACCUMULATION_PURSE_KEY.to_string())
                .map_err(|err| ProtocolUpgradeError::Bytesrepr(err.to_string()))?;

        let requries_accumulation_purse = self
            .tracking_copy
            .read(&Key::NamedKey(named_key_addr))
            .map_err(|_| ProtocolUpgradeError::UnexpectedStoredValueVariant)?
            .is_none();

        if requries_accumulation_purse {
            let purse_uref = address_generator.new_uref(AccessRights::READ_ADD_WRITE);
            let balance_clvalue = CLValue::from_t(U512::zero())?;
            self.tracking_copy.write(
                Key::Balance(purse_uref.addr()),
                StoredValue::CLValue(balance_clvalue),
            );

            let purse_key = Key::URef(purse_uref);

            self.tracking_copy
                .write(purse_key, StoredValue::CLValue(CLValue::unit()));

            let purse =
                NamedKeyValue::from_concrete_values(purse_key, ACCUMULATION_PURSE_KEY.to_string())
                    .map_err(|cl_error| ProtocolUpgradeError::CLValue(cl_error.to_string()))?;

            self.tracking_copy
                .write(Key::NamedKey(named_key_addr), StoredValue::NamedKey(purse));

            let entity_key = Key::AddressableEntity(EntityAddr::System(*handle_payment_hash));

            self.tracking_copy.write(
                entity_key,
                StoredValue::AddressableEntity(addressable_entity),
            );
        }

        Ok(())
    }

    /// Creates an accumulation purse in the handle payment system contract if its not present.
    ///
    /// This can happen on older networks that did not have support for [`FeeHandling::Accumulate`]
    /// at the genesis. In such cases we have to check the state of handle payment contract and
    /// create an accumulation purse.
    pub fn create_accumulation_purse_if_required_by_contract(
        &mut self,
        handle_payment_hash: &HashAddr,
        fee_handling: FeeHandling,
    ) -> Result<(), ProtocolUpgradeError> {
        match fee_handling {
            FeeHandling::PayToProposer | FeeHandling::Burn => return Ok(()),
            FeeHandling::Accumulate | FeeHandling::NoFee => {}
        }

        let mut address_generator = {
            let seed_bytes = (
                self.config.current_protocol_version(),
                self.config.new_protocol_version(),
            )
                .to_bytes()?;

            let phase = Phase::System;

            AddressGenerator::new(&seed_bytes, phase)
        };

        let system_contract = SystemEntityType::HandlePayment;
        let contract_name = system_contract.entity_name();
        let mut contract = if let StoredValue::Contract(contract) = self
            .tracking_copy
            .read(&Key::Hash(*handle_payment_hash))
            .map_err(|_| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(contract_name.to_string())
            })?
            .ok_or_else(|| {
                ProtocolUpgradeError::UnableToRetrieveSystemContract(contract_name.to_string())
            })? {
            contract
        } else {
            return Err(ProtocolUpgradeError::UnableToRetrieveSystemContract(
                contract_name,
            ));
        };

        if !contract.named_keys().contains(ACCUMULATION_PURSE_KEY) {
            let purse_uref = address_generator.new_uref(AccessRights::READ_ADD_WRITE);
            let balance_clvalue = CLValue::from_t(U512::zero())?;
            self.tracking_copy.write(
                Key::Balance(purse_uref.addr()),
                StoredValue::CLValue(balance_clvalue),
            );
            self.tracking_copy
                .write(Key::URef(purse_uref), StoredValue::CLValue(CLValue::unit()));

            let mut new_named_keys = NamedKeys::new();
            new_named_keys.insert(ACCUMULATION_PURSE_KEY.into(), Key::from(purse_uref));
            contract.named_keys_append(new_named_keys);

            self.tracking_copy.write(
                Key::Hash(*handle_payment_hash),
                StoredValue::Contract(contract),
            );
        }

        Ok(())
    }

    fn get_named_keys(
        &mut self,
        contract_hash: HashAddr,
    ) -> Result<NamedKeys, ProtocolUpgradeError> {
        if self.config.enable_addressable_entity() {
            let named_keys = self
                .tracking_copy
                .get_named_keys(EntityAddr::System(contract_hash))?;
            Ok(named_keys)
        } else {
            let named_keys = self
                .tracking_copy
                .read(&Key::Hash(contract_hash))?
                .ok_or_else(|| {
                    ProtocolUpgradeError::UnableToRetrieveSystemContract(format!(
                        "{:?}",
                        contract_hash
                    ))
                })?
                .as_contract()
                .map(|contract| contract.named_keys().clone())
                .ok_or_else(|| ProtocolUpgradeError::UnexpectedStoredValueVariant)?;

            Ok(named_keys)
        }
    }

    /// Check payment purse balance.
    pub fn handle_payment_purse_check(
        &mut self,
        handle_payment: HashAddr,
        mint: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        let payment_named_keys = self.get_named_keys(handle_payment)?;
        let payment_purse_key = payment_named_keys
            .get(PAYMENT_PURSE_KEY)
            .expect("payment purse key must exist in handle payment contract's named keys");
        let balance = self
            .tracking_copy
            .get_total_balance(*payment_purse_key)
            .expect("must be able to get payment purse balance");
        if balance <= Motes::zero() {
            return Ok(());
        }
        warn!("payment purse had remaining balance at upgrade {}", balance);
        let balance_key = {
            let uref_addr = payment_purse_key
                .as_uref()
                .expect("payment purse key must be uref.")
                .addr();
            Key::Balance(uref_addr)
        };

        let mint_named_keys = self.get_named_keys(mint)?;
        let total_supply_key = mint_named_keys
            .get(TOTAL_SUPPLY_KEY)
            .expect("total supply key must exist in mint contract's named keys");

        let stored_value = self
            .tracking_copy
            .read(total_supply_key)
            .expect("must be able to read total supply")
            .expect("total supply must have a value");

        // by convention, we only store CLValues under Key::URef
        if let StoredValue::CLValue(value) = stored_value {
            // Only CLTyped instances should be stored as a CLValue.
            let total_supply: U512 =
                CLValue::into_t(value).expect("total supply must have expected type.");

            let new_total_supply = total_supply.saturating_sub(balance.value());
            info!(
                "adjusting total supply from {} to {}",
                total_supply, new_total_supply
            );
            let cl_value = CLValue::from_t(new_total_supply)
                .expect("new total supply must convert to CLValue.");
            self.tracking_copy
                .write(*total_supply_key, StoredValue::CLValue(cl_value));
            info!(
                "adjusting payment purse balance from {} to {}",
                balance.value(),
                U512::zero()
            );
            let cl_value = CLValue::from_t(U512::zero()).expect("zero must convert to CLValue.");
            self.tracking_copy
                .write(balance_key, StoredValue::CLValue(cl_value));
            Ok(())
        } else {
            Err(ProtocolUpgradeError::CLValue(
                "failure to retrieve total supply".to_string(),
            ))
        }
    }

    /// Upsert gas hold interval to mint named keys.
    pub fn handle_new_gas_hold_config(
        &mut self,
        mint: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        if self.config.new_gas_hold_handling().is_none()
            && self.config.new_gas_hold_interval().is_none()
        {
            return Ok(());
        }

        let mint_addr = EntityAddr::System(mint);
        let named_keys = self.get_named_keys(mint)?;

        if let Some(new_gas_hold_handling) = self.config.new_gas_hold_handling() {
            debug!(%new_gas_hold_handling, "handle new gas hold handling");
            let stored_value =
                StoredValue::CLValue(CLValue::from_t(new_gas_hold_handling.tag()).map_err(
                    |_| ProtocolUpgradeError::Bytesrepr("new_gas_hold_handling".to_string()),
                )?);

            self.system_uref(
                mint_addr,
                MINT_GAS_HOLD_HANDLING_KEY,
                &named_keys,
                stored_value,
            )?;
        }

        if let Some(new_gas_hold_interval) = self.config.new_gas_hold_interval() {
            debug!(%new_gas_hold_interval, "handle new gas hold interval");
            let stored_value =
                StoredValue::CLValue(CLValue::from_t(new_gas_hold_interval).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_gas_hold_interval".to_string())
                })?);

            self.system_uref(
                mint_addr,
                MINT_GAS_HOLD_INTERVAL_KEY,
                &named_keys,
                stored_value,
            )?;
        }
        Ok(())
    }

    fn system_uref(
        &mut self,
        entity_addr: EntityAddr,
        name: &str,
        named_keys: &NamedKeys,
        stored_value: StoredValue,
    ) -> Result<(), ProtocolUpgradeError> {
        let uref = {
            match named_keys.get(name) {
                Some(key) => match key.as_uref() {
                    Some(uref) => *uref,
                    None => {
                        return Err(ProtocolUpgradeError::UnexpectedKeyVariant);
                    }
                },
                None => self
                    .address_generator
                    .borrow_mut()
                    .new_uref(AccessRights::READ_ADD_WRITE),
            }
        };
        self.tracking_copy
            .upsert_uref_to_named_keys(entity_addr, name, named_keys, uref, stored_value)
            .map_err(ProtocolUpgradeError::TrackingCopy)
    }

    /// Handle new validator slots.
    pub fn handle_new_validator_slots(
        &mut self,
        auction: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_validator_slots) = self.config.new_validator_slots() {
            debug!(%new_validator_slots, "handle new validator slots");
            // if new total validator slots is provided, update auction contract state
            let auction_named_keys = self.get_named_keys(auction)?;

            let validator_slots_key = auction_named_keys
                .get(VALIDATOR_SLOTS_KEY)
                .expect("validator_slots key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_validator_slots).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_validator_slots".to_string())
                })?);
            self.tracking_copy.write(*validator_slots_key, value);
        }
        Ok(())
    }

    /// Applies the necessary changes if a new auction delay is part of the upgrade.
    pub fn handle_new_auction_delay(
        &mut self,
        auction: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_auction_delay) = self.config.new_auction_delay() {
            debug!(%new_auction_delay, "handle new auction delay");
            let auction_named_keys = self.get_named_keys(auction)?;

            let auction_delay_key = auction_named_keys
                .get(AUCTION_DELAY_KEY)
                .expect("auction_delay key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_auction_delay).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_auction_delay".to_string())
                })?);
            self.tracking_copy.write(*auction_delay_key, value);
        }
        Ok(())
    }

    /// Applies the necessary changes if a new locked funds period is part of the upgrade.
    pub fn handle_new_locked_funds_period_millis(
        &mut self,
        auction: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_locked_funds_period) = self.config.new_locked_funds_period_millis() {
            debug!(%new_locked_funds_period,"handle new locked funds period millis");

            let auction_named_keys = self.get_named_keys(auction)?;

            let locked_funds_period_key = auction_named_keys
                .get(LOCKED_FUNDS_PERIOD_KEY)
                .expect("locked_funds_period key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_locked_funds_period).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_locked_funds_period".to_string())
                })?);
            self.tracking_copy.write(*locked_funds_period_key, value);
        }
        Ok(())
    }

    /// Applies the necessary changes if a new unbonding delay is part of the upgrade.
    pub fn handle_new_unbonding_delay(
        &mut self,
        auction: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        // We insert the new unbonding delay once the purses to be paid out have been transformed
        // based on the previous unbonding delay.
        if let Some(new_unbonding_delay) = self.config.new_unbonding_delay() {
            debug!(%new_unbonding_delay,"handle new unbonding delay");

            let auction_named_keys = self.get_named_keys(auction)?;

            let unbonding_delay_key = auction_named_keys
                .get(UNBONDING_DELAY_KEY)
                .expect("unbonding_delay key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_unbonding_delay).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_unbonding_delay".to_string())
                })?);
            self.tracking_copy.write(*unbonding_delay_key, value);
        }
        Ok(())
    }

    /// Applies the necessary changes if a new round seigniorage rate is part of the upgrade.
    pub fn handle_new_round_seigniorage_rate(
        &mut self,
        mint: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_round_seigniorage_rate) = self.config.new_round_seigniorage_rate() {
            debug!(%new_round_seigniorage_rate,"handle new round seigniorage rate");
            let new_round_seigniorage_rate: Ratio<U512> = {
                let (numer, denom) = new_round_seigniorage_rate.into();
                Ratio::new(numer.into(), denom.into())
            };

            let mint_named_keys = self.get_named_keys(mint)?;

            let locked_funds_period_key = mint_named_keys
                .get(ROUND_SEIGNIORAGE_RATE_KEY)
                .expect("round_seigniorage_rate key must exist in mint contract's named keys");
            let value = StoredValue::CLValue(CLValue::from_t(new_round_seigniorage_rate).map_err(
                |_| ProtocolUpgradeError::Bytesrepr("new_round_seigniorage_rate".to_string()),
            )?);
            self.tracking_copy.write(*locked_funds_period_key, value);
        }
        Ok(())
    }

    /// Handle unbonds migration.
    pub fn handle_unbonds_migration(&mut self) -> Result<(), ProtocolUpgradeError> {
        debug!("handle unbonds migration");
        let tc = &mut self.tracking_copy;
        let existing_keys = match tc.get_keys(&KeyTag::Unbond) {
            Ok(keys) => keys,
            Err(err) => return Err(ProtocolUpgradeError::TrackingCopy(err)),
        };
        for key in existing_keys {
            if let Some(StoredValue::Unbonding(unbonding_purses)) =
                tc.get(&key).map_err(Into::<ProtocolUpgradeError>::into)?
            {
                // prune away the original record, we don't need it anymore
                tc.prune(key);

                // re-write records under Key::BidAddr , StoredValue::BidKind
                for unbonding_purse in unbonding_purses {
                    let validator = unbonding_purse.validator_public_key();
                    let unbonder = unbonding_purse.unbonder_public_key();
                    let new_key = Key::BidAddr(BidAddr::UnbondAccount {
                        validator: validator.to_account_hash(),
                        unbonder: unbonder.to_account_hash(),
                    });
                    let unbond = Box::new(Unbond::from(unbonding_purse));
                    let unbond_bid_kind = BidKind::Unbond(unbond.clone());
                    if !unbond.eras().is_empty() {
                        tc.write(new_key, StoredValue::BidKind(unbond_bid_kind));
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle bids migration.
    pub fn handle_bids_migration(
        &mut self,
        chainspec_minimum: u64,
        chainspec_maximum: u64,
    ) -> Result<(), ProtocolUpgradeError> {
        if chainspec_maximum < chainspec_minimum {
            return Err(ProtocolUpgradeError::InvalidUpgradeConfig);
        }
        debug!("handle bids migration");
        let tc = &mut self.tracking_copy;
        let existing_bid_keys = match tc.get_keys(&KeyTag::Bid) {
            Ok(keys) => keys,
            Err(err) => return Err(ProtocolUpgradeError::TrackingCopy(err)),
        };
        for key in existing_bid_keys {
            if let Some(StoredValue::Bid(existing_bid)) =
                tc.get(&key).map_err(Into::<ProtocolUpgradeError>::into)?
            {
                // prune away the original record, we don't need it anymore
                tc.prune(key);

                if existing_bid.staked_amount().is_zero() {
                    // the previous logic enforces unbonding all delegators of
                    // a validator that reduced their personal stake to 0 (and we have
                    // various existent tests that prove this), thus there is no need
                    // to handle the complicated hypothetical case of one or more
                    // delegator stakes being > 0 if the validator stake is 0.
                    //
                    // tl;dr this is a "zombie" bid and we don't need to continue
                    // carrying it forward at tip.
                    continue;
                }

                let validator_public_key = existing_bid.validator_public_key();
                let validator_bid_addr = BidAddr::from(validator_public_key.clone());
                let validator_bid = {
                    let validator_bid = ValidatorBid::from(*existing_bid.clone());
                    validator_bid
                        .with_min_max_delegation_amount(chainspec_maximum, chainspec_minimum)
                };
                tc.write(
                    validator_bid_addr.into(),
                    StoredValue::BidKind(BidKind::Validator(Box::new(validator_bid))),
                );

                let delegators = existing_bid.delegators().clone();
                for (_, delegator) in delegators {
                    let delegator_bid_addr = BidAddr::new_delegator_kind(
                        validator_public_key,
                        &DelegatorKind::PublicKey(delegator.delegator_public_key().clone()),
                    );
                    // the previous code was removing a delegator bid from the embedded
                    // collection within their validator's bid when the delegator fully
                    // unstaked, so technically we don't need to check for 0 balance here.
                    // However, since it is low effort to check, doing it just to be sure.
                    if !delegator.staked_amount().is_zero() {
                        tc.write(
                            delegator_bid_addr.into(),
                            StoredValue::BidKind(BidKind::Delegator(Box::new(DelegatorBid::from(
                                delegator,
                            )))),
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle era info migration.
    pub fn handle_era_info_migration(&mut self) -> Result<(), ProtocolUpgradeError> {
        // EraInfo migration
        if let Some(activation_point) = self.config.activation_point() {
            // The highest stored era is the immediate predecessor of the activation point.
            let highest_era_info_id = activation_point.saturating_sub(1);
            let highest_era_info_key = Key::EraInfo(highest_era_info_id);

            let get_result = self
                .tracking_copy
                .get(&highest_era_info_key)
                .map_err(ProtocolUpgradeError::TrackingCopy)?;

            match get_result {
                Some(stored_value @ StoredValue::EraInfo(_)) => {
                    self.tracking_copy.write(Key::EraSummary, stored_value);
                }
                Some(other_stored_value) => {
                    // This should not happen as we only write EraInfo variants.
                    error!(stored_value_type_name=%other_stored_value.type_name(),
                        "EraInfo key contains unexpected StoredValue variant");
                    return Err(ProtocolUpgradeError::UnexpectedStoredValueVariant);
                }
                None => {
                    // Can't find key
                    // Most likely this chain did not yet run an auction, or recently completed a
                    // prune
                }
            };
        }
        Ok(())
    }

    /// Handle seignorage snapshot migration to new version.
    pub fn handle_seignorage_snapshot_migration(
        &mut self,
        auction: HashAddr,
    ) -> Result<(), ProtocolUpgradeError> {
        let auction_named_keys = self.get_named_keys(auction)?;
        let maybe_snapshot_version_key =
            auction_named_keys.get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY);
        let snapshot_key = auction_named_keys
            .get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
            .expect("snapshot key should already exist");

        // if version flag does not exist yet, set it and migrate snapshot
        if maybe_snapshot_version_key.is_none() {
            let auction_addr = EntityAddr::new_system(auction);

            // add new snapshot version named key
            let stored_value = StoredValue::CLValue(CLValue::from_t(
                DEFAULT_SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION,
            )?);
            self.system_uref(
                auction_addr,
                SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY,
                &auction_named_keys,
                stored_value,
            )?;

            // read legacy snapshot
            if let Some(snapshot_stored_value) = self.tracking_copy.read(snapshot_key)? {
                let snapshot_cl_value = match snapshot_stored_value.into_cl_value() {
                    Some(cl_value) => cl_value,
                    None => {
                        error!("seigniorage recipients snapshot is not a CLValue");
                        return Err(ProtocolUpgradeError::CLValue(
                            "seigniorage recipients snapshot is not a CLValue".to_string(),
                        ));
                    }
                };

                let legacy_snapshot: SeigniorageRecipientsSnapshotV1 =
                    snapshot_cl_value.into_t()?;

                let mut new_snapshot = SeigniorageRecipientsSnapshotV2::default();
                for (era_id, recipients) in legacy_snapshot.into_iter() {
                    let mut new_recipients = SeigniorageRecipientsV2::default();
                    for (pubkey, recipient) in recipients {
                        new_recipients.insert(pubkey, recipient.into());
                    }
                    new_snapshot.insert(era_id, new_recipients);
                }

                // store new snapshot
                self.tracking_copy.write(
                    *snapshot_key,
                    StoredValue::CLValue(CLValue::from_t(new_snapshot)?),
                );
            };
        }

        Ok(())
    }

    /// Handle global state updates.
    pub fn handle_global_state_updates(&mut self) {
        debug!("handle global state updates");
        for (key, value) in self.config.global_state_update() {
            self.tracking_copy.write(*key, value.clone());
        }
    }
}
