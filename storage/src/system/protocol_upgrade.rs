//! Support for applying upgrades on the execution engine.
use num_rational::Ratio;
use std::{cell::RefCell, collections::BTreeSet, fmt, rc::Rc};

use thiserror::Error;
use tracing::{debug, error};

use casper_types::{
    addressable_entity::{
        ActionThresholds, AssociatedKeys, EntityKind, EntityKindTag, MessageTopics, NamedKeyAddr,
        NamedKeyValue, NamedKeys, Weight,
    },
    bytesrepr::{self, ToBytes},
    execution::Effects,
    package::{EntityVersions, Groups, PackageStatus},
    system::{
        auction::{
            BidAddr, BidKind, ValidatorBid, AUCTION_DELAY_KEY, LOCKED_FUNDS_PERIOD_KEY,
            UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        handle_payment::ACCUMULATION_PURSE_KEY,
        mint::ROUND_SEIGNIORAGE_RATE_KEY,
        SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT,
    },
    AccessRights, AddressableEntity, AddressableEntityHash, ByteCode, ByteCodeAddr, ByteCodeHash,
    ByteCodeKind, CLValue, CLValueError, Digest, EntityAddr, EntryPoints, FeeHandling, Key, KeyTag,
    Package, PackageHash, Phase, ProtocolUpgradeConfig, ProtocolVersion, PublicKey, StoredValue,
    SystemEntityRegistry, URef, U512,
};

use crate::{
    global_state::state::StateProvider,
    tracking_copy::{TrackingCopy, TrackingCopyExt},
    AddressGenerator,
};

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
    /// Failed to create system contract registry.
    #[error("Failed to insert system contract registry")]
    FailedToCreateSystemRegistry,
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
    TrackingCopyError(crate::tracking_copy::TrackingCopyError),
}

impl From<CLValueError> for ProtocolUpgradeError {
    fn from(v: CLValueError) -> Self {
        Self::CLValue(v.to_string())
    }
}

impl From<crate::tracking_copy::TrackingCopyError> for ProtocolUpgradeError {
    fn from(err: crate::tracking_copy::TrackingCopyError) -> Self {
        ProtocolUpgradeError::TrackingCopyError(err)
    }
}

impl From<bytesrepr::Error> for ProtocolUpgradeError {
    fn from(error: bytesrepr::Error) -> Self {
        ProtocolUpgradeError::Bytesrepr(error.to_string())
    }
}

/// Adrresses for system entities.
pub struct SystemEntityAddresses {
    mint: AddressableEntityHash,
    auction: AddressableEntityHash,
    handle_payment: AddressableEntityHash,
}

impl SystemEntityAddresses {
    /// Creates a new instance of system entity addresses.
    pub fn new(
        mint: AddressableEntityHash,
        auction: AddressableEntityHash,
        handle_payment: AddressableEntityHash,
    ) -> Self {
        SystemEntityAddresses {
            mint,
            auction,
            handle_payment,
        }
    }

    /// Mint address.
    pub fn mint(&self) -> AddressableEntityHash {
        self.mint
    }

    /// Auction address.
    pub fn auction(&self) -> AddressableEntityHash {
        self.auction
    }

    /// Handle payment address.
    pub fn handle_payment(&self) -> AddressableEntityHash {
        self.handle_payment
    }
}

/// The system upgrader deals with conducting an actual protocol upgrade.
pub struct ProtocolUpgrader<S>
where
    S: StateProvider + ?Sized,
{
    config: ProtocolUpgradeConfig,
    tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
}

impl<S> ProtocolUpgrader<S>
where
    S: StateProvider + ?Sized,
{
    /// Creates new system upgrader instance.
    pub fn new(
        config: ProtocolUpgradeConfig,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Self {
        ProtocolUpgrader {
            config,
            tracking_copy,
        }
    }

    pub fn upgrade(self, pre_state_hash: Digest) -> Result<(), ProtocolUpgradeError> {
        self.check_next_protocol_version_validity()?;
        let system_entity_addresses = self.handle_system_entities()?;
        self.migrate_system_account(pre_state_hash)?;
        self.create_accumulation_purse_if_required(
            &system_entity_addresses.handle_payment(),
            self.config.fee_handling(),
        )?;
        self.refresh_system_contracts(&system_entity_addresses)?;
        self.handle_new_validator_slots(system_entity_addresses.auction())?;
        self.handle_new_auction_delay(system_entity_addresses.auction())?;
        self.handle_new_locked_funds_period_millis(system_entity_addresses.auction())?;
        self.handle_new_unbonding_delay(system_entity_addresses.auction())?;
        self.handle_new_round_seigniorage_rate(system_entity_addresses.mint())?;
        self.handle_bids_migration()?;
        self.handle_global_state_updates();
        self.handle_era_info_migration()
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

    fn system_contract_registry(&self) -> Result<SystemEntityRegistry, ProtocolUpgradeError> {
        debug!("system contract registry");
        let registry = if let Ok(registry) = self.tracking_copy.borrow_mut().get_system_contracts()
        {
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
                CLValue::into_t::<SystemEntityRegistry>(cl_registry).map_err(|error| {
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
    pub fn handle_system_entities(&self) -> Result<SystemEntityAddresses, ProtocolUpgradeError> {
        debug!("handle system entities");
        let mut registry = self.system_contract_registry()?;

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

            self.tracking_copy.borrow_mut().write(
                Key::SystemEntityRegistry,
                StoredValue::CLValue(cl_value_chainspec_registry),
            );

            // Prune away standard payment from global state.
            self.tracking_copy
                .borrow_mut()
                .prune(Key::Hash(standard_payment_hash.value()));
        };

        // Write the chainspec registry to global state
        let cl_value_chainspec_registry = CLValue::from_t(self.config.chainspec_registry().clone())
            .map_err(|error| ProtocolUpgradeError::Bytesrepr(error.to_string()))?;

        self.tracking_copy.borrow_mut().write(
            Key::ChainspecRegistry,
            StoredValue::CLValue(cl_value_chainspec_registry),
        );

        let system_entity_addresses = SystemEntityAddresses::new(mint, auction, handle_payment);

        Ok(system_entity_addresses)
    }

    /// Bump major version and/or update the entry points for system contracts.
    pub fn refresh_system_contracts(
        &self,
        system_entity_addresses: &SystemEntityAddresses,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!("refresh system contracts");
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
    fn refresh_system_contract_entry_points(
        &self,
        contract_hash: AddressableEntityHash,
        system_contract_type: SystemEntityType,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!(%system_contract_type, "refresh system contract entry points");
        let contract_name = system_contract_type.contract_name();
        let entry_points = system_contract_type.contract_entry_points();

        let (mut contract, maybe_named_keys, must_prune) =
            self.retrieve_system_entity(contract_hash, system_contract_type)?;

        let mut package =
            self.retrieve_system_package(contract.package_hash(), system_contract_type)?;

        package.disable_entity_version(contract_hash).map_err(|_| {
            ProtocolUpgradeError::FailedToDisablePreviousVersion(contract_name.to_string())
        })?;

        contract.set_protocol_version(self.config.new_protocol_version());

        let new_entity = AddressableEntity::new(
            contract.package_hash(),
            ByteCodeHash::default(),
            entry_points,
            self.config.new_protocol_version(),
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
                    .map_err(|err| ProtocolUpgradeError::Bytesrepr(err.to_string()))?;

                let named_key_value = NamedKeyValue::from_concrete_values(key, string)
                    .map_err(|error| ProtocolUpgradeError::CLValue(error.to_string()))?;

                let entry_key = Key::NamedKey(entry_addr);

                self.tracking_copy
                    .borrow_mut()
                    .write(entry_key, StoredValue::NamedKey(named_key_value));
            }
        }

        package.insert_entity_version(
            self.config.new_protocol_version().value().major,
            contract_hash,
        );

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
        debug!(%system_contract_type, "retrieve system package");
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

    fn retrieve_system_entity(
        &self,
        contract_hash: AddressableEntityHash,
        system_contract_type: SystemEntityType,
    ) -> Result<(AddressableEntity, Option<NamedKeys>, bool), ProtocolUpgradeError> {
        debug!(%system_contract_type, "retrieve system entity");
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

    pub fn migrate_system_account(
        &self,
        pre_state_hash: Digest,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!("migrate system account");
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
            self.config.new_protocol_version(),
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
            package.insert_entity_version(
                self.config.new_protocol_version().value().major,
                entity_hash,
            );
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
    pub fn create_accumulation_purse_if_required(
        &self,
        handle_payment_hash: &AddressableEntityHash,
        fee_handling: FeeHandling,
    ) -> Result<(), ProtocolUpgradeError> {
        debug!(?fee_handling, "create accumulation purse if required");
        match fee_handling {
            FeeHandling::PayToProposer | FeeHandling::Burn => return Ok(()),
            FeeHandling::Accumulate => {}
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

        let entity_addr = EntityAddr::new_system_entity_addr(handle_payment_hash.value());

        if let Some(named_keys) = maybe_named_keys {
            for (string, key) in named_keys.into_inner().into_iter() {
                let entry_addr = NamedKeyAddr::new_from_string(entity_addr, string.clone())
                    .map_err(|err| ProtocolUpgradeError::Bytesrepr(err.to_string()))?;

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
                .map_err(|err| ProtocolUpgradeError::Bytesrepr(err.to_string()))?;

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

    /// Handle new validator slots.
    pub fn handle_new_validator_slots(
        &self,
        auction: AddressableEntityHash,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_validator_slots) = self.config.new_validator_slots() {
            debug!(%new_validator_slots, "handle new validator slots");
            // if new total validator slots is provided, update auction contract state
            let auction_addr = EntityAddr::new_system_entity_addr(auction.value());
            let auction_named_keys = self
                .tracking_copy
                .borrow_mut()
                .get_named_keys(auction_addr)?;

            let validator_slots_key = auction_named_keys
                .get(VALIDATOR_SLOTS_KEY)
                .expect("validator_slots key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_validator_slots).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_validator_slots".to_string())
                })?);
            self.tracking_copy
                .borrow_mut()
                .write(*validator_slots_key, value);
        }
        Ok(())
    }

    pub fn handle_new_auction_delay(
        &self,
        auction: AddressableEntityHash,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_auction_delay) = self.config.new_auction_delay() {
            debug!(%new_auction_delay, "handle new auction delay");
            let auction_addr = EntityAddr::new_system_entity_addr(auction.value());

            let auction_named_keys = self
                .tracking_copy
                .borrow_mut()
                .get_named_keys(auction_addr)?;

            let auction_delay_key = auction_named_keys
                .get(AUCTION_DELAY_KEY)
                .expect("auction_delay key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_auction_delay).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_auction_delay".to_string())
                })?);
            self.tracking_copy
                .borrow_mut()
                .write(*auction_delay_key, value);
        }
        Ok(())
    }

    pub fn handle_new_locked_funds_period_millis(
        &self,
        auction: AddressableEntityHash,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_locked_funds_period) = self.config.new_locked_funds_period_millis() {
            debug!(%new_locked_funds_period,"handle new locked funds period millis");
            let auction_addr = EntityAddr::new_system_entity_addr(auction.value());

            let auction_named_keys = self
                .tracking_copy
                .borrow_mut()
                .get_named_keys(auction_addr)?;

            let locked_funds_period_key = auction_named_keys
                .get(LOCKED_FUNDS_PERIOD_KEY)
                .expect("locked_funds_period key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_locked_funds_period).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_locked_funds_period".to_string())
                })?);
            self.tracking_copy
                .borrow_mut()
                .write(*locked_funds_period_key, value);
        }
        Ok(())
    }

    pub fn handle_new_unbonding_delay(
        &self,
        auction: AddressableEntityHash,
    ) -> Result<(), ProtocolUpgradeError> {
        // We insert the new unbonding delay once the purses to be paid out have been transformed
        // based on the previous unbonding delay.
        if let Some(new_unbonding_delay) = self.config.new_unbonding_delay() {
            debug!(%new_unbonding_delay,"handle new unbonding delay");
            let auction_addr = EntityAddr::new_system_entity_addr(auction.value());

            let auction_named_keys = self
                .tracking_copy
                .borrow_mut()
                .get_named_keys(auction_addr)?;

            let unbonding_delay_key = auction_named_keys
                .get(UNBONDING_DELAY_KEY)
                .expect("unbonding_delay key must exist in auction contract's named keys");
            let value =
                StoredValue::CLValue(CLValue::from_t(new_unbonding_delay).map_err(|_| {
                    ProtocolUpgradeError::Bytesrepr("new_unbonding_delay".to_string())
                })?);
            self.tracking_copy
                .borrow_mut()
                .write(*unbonding_delay_key, value);
        }
        Ok(())
    }

    pub fn handle_new_round_seigniorage_rate(
        &self,
        mint: AddressableEntityHash,
    ) -> Result<(), ProtocolUpgradeError> {
        if let Some(new_round_seigniorage_rate) = self.config.new_round_seigniorage_rate() {
            debug!(%new_round_seigniorage_rate,"handle new round seigniorage rate");
            let new_round_seigniorage_rate: Ratio<U512> = {
                let (numer, denom) = new_round_seigniorage_rate.into();
                Ratio::new(numer.into(), denom.into())
            };

            let mint_addr = EntityAddr::new_system_entity_addr(mint.value());

            let mint_named_keys = self.tracking_copy.borrow_mut().get_named_keys(mint_addr)?;

            let locked_funds_period_key = mint_named_keys
                .get(ROUND_SEIGNIORAGE_RATE_KEY)
                .expect("round_seigniorage_rate key must exist in mint contract's named keys");
            let value = StoredValue::CLValue(CLValue::from_t(new_round_seigniorage_rate).map_err(
                |_| ProtocolUpgradeError::Bytesrepr("new_round_seigniorage_rate".to_string()),
            )?);
            self.tracking_copy
                .borrow_mut()
                .write(*locked_funds_period_key, value);
        }
        Ok(())
    }

    /// Handle bids migration.
    pub fn handle_bids_migration(&self) -> Result<(), ProtocolUpgradeError> {
        debug!("handle bids migration");
        let mut borrow = self.tracking_copy.borrow_mut();
        let existing_bid_keys = match borrow.get_keys(&KeyTag::Bid) {
            Ok(keys) => keys,
            Err(err) => return Err(ProtocolUpgradeError::TrackingCopyError(err)),
        };
        for key in existing_bid_keys {
            if let Some(StoredValue::Bid(existing_bid)) = borrow
                .get(&key)
                .map_err(Into::<ProtocolUpgradeError>::into)?
            {
                // prune away the original record, we don't need it anymore
                borrow.prune(key);

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
                let validator_bid = ValidatorBid::from(*existing_bid.clone());
                borrow.write(
                    validator_bid_addr.into(),
                    StoredValue::BidKind(BidKind::Validator(Box::new(validator_bid))),
                );

                let delegators = existing_bid.delegators().clone();
                for (_, delegator) in delegators {
                    let delegator_bid_addr = BidAddr::new_from_public_keys(
                        validator_public_key,
                        Some(delegator.delegator_public_key()),
                    );
                    // the previous code was removing a delegator bid from the embedded
                    // collection within their validator's bid when the delegator fully
                    // unstaked, so technically we don't need to check for 0 balance here.
                    // However, since it is low effort to check, doing it just to be sure.
                    if !delegator.staked_amount().is_zero() {
                        borrow.write(
                            delegator_bid_addr.into(),
                            StoredValue::BidKind(BidKind::Delegator(Box::new(delegator))),
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle era info migration.
    pub fn handle_era_info_migration(&self) -> Result<(), ProtocolUpgradeError> {
        // EraInfo migration
        if let Some(activation_point) = self.config.activation_point() {
            // The highest stored era is the immediate predecessor of the activation point.
            let highest_era_info_id = activation_point.saturating_sub(1);
            let highest_era_info_key = Key::EraInfo(highest_era_info_id);

            let get_result = self
                .tracking_copy
                .borrow_mut()
                .get(&highest_era_info_key)
                .map_err(ProtocolUpgradeError::TrackingCopyError)?;

            match get_result {
                Some(stored_value @ StoredValue::EraInfo(_)) => {
                    self.tracking_copy
                        .borrow_mut()
                        .write(Key::EraSummary, stored_value);
                }
                Some(other_stored_value) => {
                    // This should not happen as we only write EraInfo variants.
                    error!(stored_value_type_name=%other_stored_value.type_name(),
                        "EraInfo key contains unexpected StoredValue variant");
                    return Err(ProtocolUpgradeError::UnexpectedStoredValueVariant);
                }
                None => {
                    // Can't find key
                    // Most likely this chain did not yet ran an auction, or recently completed a
                    // prune
                }
            };
        }
        Ok(())
    }

    /// Handle global state updates.
    pub fn handle_global_state_updates(&self) {
        debug!("handle global state updates");
        for (key, value) in self.config.global_state_update() {
            self.tracking_copy.borrow_mut().write(*key, value.clone());
        }
    }
}
