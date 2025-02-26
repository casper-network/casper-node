use std::collections::BTreeSet;
use tracing::{debug, error};

use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, NamedKeyAddr, NamedKeyValue, Weight},
    contracts::{ContractHash, NamedKeys},
    system::{
        handle_payment::ACCUMULATION_PURSE_KEY, SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT,
    },
    AccessRights, Account, AddressableEntity, AddressableEntityHash, ByteCode, ByteCodeAddr,
    ByteCodeHash, CLValue, ContextAccessRights, ContractRuntimeTag, EntityAddr, EntityKind,
    EntityVersions, EntryPointAddr, EntryPointValue, EntryPoints, Groups, HashAddr, Key, Package,
    PackageHash, PackageStatus, Phase, ProtocolVersion, PublicKey, RuntimeFootprint, StoredValue,
    StoredValueTypeMismatch, URef, U512,
};

use crate::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyError, TrackingCopyExt},
    AddressGenerator, KeyPrefix,
};

/// Fees purse handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeesPurseHandling {
    /// Transfer fees to proposer.
    ToProposer(AccountHash),
    /// Transfer all fees to a system-wide accumulation purse, for future disbursement.
    Accumulate,
    /// Burn all fees from specified purse.
    Burn(URef),
    /// No fees are charged.
    None(URef),
}

/// Higher-level operations on the state via a `TrackingCopy`.
pub trait TrackingCopyEntityExt<R> {
    /// The type for the returned errors.
    type Error;

    /// Gets a runtime information by entity_addr.
    fn runtime_footprint_by_entity_addr(
        &self,
        entity_addr: EntityAddr,
    ) -> Result<RuntimeFootprint, Self::Error>;

    /// Gets a runtime information by hash_addr.
    fn runtime_footprint_by_hash_addr(
        &mut self,
        hash_addr: HashAddr,
    ) -> Result<RuntimeFootprint, Self::Error>;

    /// Gets a runtime information by account hash.
    fn runtime_footprint_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<(EntityAddr, RuntimeFootprint), Self::Error>;

    /// Get runtime information for an account if authorized, else error.
    fn authorized_runtime_footprint_by_account(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        administrative_accounts: &BTreeSet<AccountHash>,
    ) -> Result<(RuntimeFootprint, EntityAddr), Self::Error>;

    /// Returns runtime information and access rights if authorized, else error.
    fn authorized_runtime_footprint_with_access_rights(
        &mut self,
        protocol_version: ProtocolVersion,
        initiating_address: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        administrative_accounts: &BTreeSet<AccountHash>,
    ) -> Result<(EntityAddr, RuntimeFootprint, ContextAccessRights), TrackingCopyError>;

    /// Returns runtime information for systemic functionality.
    fn system_entity_runtime_footprint(
        &mut self,
        protocol_version: ProtocolVersion,
    ) -> Result<(EntityAddr, RuntimeFootprint, ContextAccessRights), TrackingCopyError>;

    /// Migrate the NamedKeys for a entity.
    fn migrate_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        named_keys: NamedKeys,
    ) -> Result<(), Self::Error>;

    /// Migrate entry points from and older structure to top level entries.
    fn migrate_entry_points(
        &mut self,
        entity_addr: EntityAddr,
        entry_points: EntryPoints,
    ) -> Result<(), Self::Error>;

    /// Upsert uref value to global state and imputed entity's named keys.
    fn upsert_uref_to_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        name: &str,
        named_keys: &NamedKeys,
        uref: URef,
        stored_value: StoredValue,
    ) -> Result<(), Self::Error>;

    /// Migrate Account to AddressableEntity.
    fn migrate_account(
        &mut self,
        account_hash: AccountHash,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error>;

    /// Create an addressable entity to receive transfer.
    fn create_new_addressable_entity_on_transfer(
        &mut self,
        account_hash: AccountHash,
        main_purse: URef,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error>;

    /// Create an addressable entity instance using the field data of an account instance.
    fn create_addressable_entity_from_account(
        &mut self,
        account: Account,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error>;

    /// Migrate ContractPackage to Package.
    fn migrate_package(
        &mut self,
        contract_package_key: Key,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error>;

    /// Returns fee purse.
    fn fees_purse(
        &mut self,
        protocol_version: ProtocolVersion,
        fees_purse_handling: FeesPurseHandling,
    ) -> Result<URef, TrackingCopyError>;

    /// Returns named key from selected system contract.
    fn system_contract_named_key(
        &mut self,
        system_contract_name: &str,
        name: &str,
    ) -> Result<Option<Key>, Self::Error>;
}

impl<R> TrackingCopyEntityExt<R> for TrackingCopy<R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    type Error = TrackingCopyError;

    fn runtime_footprint_by_entity_addr(
        &self,
        entity_addr: EntityAddr,
    ) -> Result<RuntimeFootprint, Self::Error> {
        let entity_key = match entity_addr {
            EntityAddr::Account(account_addr) => {
                let account_key = Key::Account(AccountHash::new(account_addr));
                match self.read(&account_key)? {
                    Some(StoredValue::Account(account)) => {
                        return Ok(RuntimeFootprint::new_account_footprint(account))
                    }
                    Some(StoredValue::CLValue(cl_value)) => cl_value.to_t::<Key>()?,
                    Some(other) => {
                        return Err(TrackingCopyError::TypeMismatch(
                            StoredValueTypeMismatch::new(
                                "Account or Key".to_string(),
                                other.type_name(),
                            ),
                        ))
                    }
                    None => return Err(TrackingCopyError::KeyNotFound(account_key)),
                }
            }
            EntityAddr::SmartContract(addr) | EntityAddr::System(addr) => {
                let contract_key = Key::Hash(addr);
                match self.read(&contract_key)? {
                    Some(StoredValue::Contract(contract)) => {
                        let contract_hash = ContractHash::new(entity_addr.value());
                        let maybe_system_entity_type = {
                            let mut ret = None;
                            let registry = self.get_system_entity_registry()?;
                            for (name, hash) in registry.inner().into_iter() {
                                if hash == entity_addr.value() {
                                    match name.as_ref() {
                                        MINT => ret = Some(SystemEntityType::Mint),
                                        AUCTION => ret = Some(SystemEntityType::Auction),
                                        HANDLE_PAYMENT => {
                                            ret = Some(SystemEntityType::HandlePayment)
                                        }
                                        _ => continue,
                                    }
                                }
                            }

                            ret
                        };

                        return Ok(RuntimeFootprint::new_contract_footprint(
                            contract_hash,
                            contract,
                            maybe_system_entity_type,
                        ));
                    }
                    Some(StoredValue::CLValue(cl_value)) => cl_value.to_t::<Key>()?,
                    Some(_) | None => Key::AddressableEntity(entity_addr),
                }
            }
        };

        match self.read(&entity_key)? {
            Some(StoredValue::AddressableEntity(entity)) => {
                let named_keys = {
                    let keys =
                        self.get_keys_by_prefix(&KeyPrefix::NamedKeysByEntity(entity_addr))?;

                    let mut named_keys = NamedKeys::new();

                    for entry_key in &keys {
                        match self.read(entry_key)? {
                            Some(StoredValue::NamedKey(named_key)) => {
                                let key =
                                    named_key.get_key().map_err(TrackingCopyError::CLValue)?;
                                let name =
                                    named_key.get_name().map_err(TrackingCopyError::CLValue)?;
                                named_keys.insert(name, key);
                            }
                            Some(other) => {
                                return Err(TrackingCopyError::TypeMismatch(
                                    StoredValueTypeMismatch::new(
                                        "CLValue".to_string(),
                                        other.type_name(),
                                    ),
                                ));
                            }
                            None => match self.cache.reads_cached.get(entry_key) {
                                Some(StoredValue::NamedKey(named_key_value)) => {
                                    let key = named_key_value
                                        .get_key()
                                        .map_err(TrackingCopyError::CLValue)?;
                                    let name = named_key_value
                                        .get_name()
                                        .map_err(TrackingCopyError::CLValue)?;
                                    named_keys.insert(name, key);
                                }
                                Some(_) | None => {
                                    return Err(TrackingCopyError::KeyNotFound(*entry_key));
                                }
                            },
                        };
                    }

                    named_keys
                };
                let entry_points = {
                    let keys =
                        self.get_keys_by_prefix(&KeyPrefix::EntryPointsV1ByEntity(entity_addr))?;

                    let mut entry_points_v1 = EntryPoints::new();

                    for entry_point_key in keys.iter() {
                        match self.read(entry_point_key)? {
                            Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(
                                entry_point,
                            ))) => entry_points_v1.add_entry_point(entry_point),
                            Some(other) => {
                                return Err(TrackingCopyError::TypeMismatch(
                                    StoredValueTypeMismatch::new(
                                        "EntryPointsV1".to_string(),
                                        other.type_name(),
                                    ),
                                ));
                            }
                            None => match self.cache.reads_cached.get(entry_point_key) {
                                Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(
                                    entry_point,
                                ))) => entry_points_v1.add_entry_point(entry_point.to_owned()),
                                Some(other) => {
                                    return Err(TrackingCopyError::TypeMismatch(
                                        StoredValueTypeMismatch::new(
                                            "EntryPointsV1".to_string(),
                                            other.type_name(),
                                        ),
                                    ));
                                }
                                None => {
                                    return Err(TrackingCopyError::KeyNotFound(*entry_point_key));
                                }
                            },
                        }
                    }

                    entry_points_v1
                };
                Ok(RuntimeFootprint::new_entity_footprint(
                    entity_addr,
                    entity,
                    named_keys,
                    entry_points,
                ))
            }
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("AddressableEntity".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(entity_key)),
        }
    }

    fn runtime_footprint_by_hash_addr(
        &mut self,
        hash_addr: HashAddr,
    ) -> Result<RuntimeFootprint, Self::Error> {
        let entity_addr = if self.get_system_entity_registry()?.exists(&hash_addr) {
            EntityAddr::new_system(hash_addr)
        } else {
            EntityAddr::new_smart_contract(hash_addr)
        };

        self.runtime_footprint_by_entity_addr(entity_addr)
    }

    fn runtime_footprint_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<(EntityAddr, RuntimeFootprint), Self::Error> {
        let account_key = Key::Account(account_hash);

        let entity_addr = match self.get(&account_key)? {
            Some(StoredValue::Account(account)) => {
                if self.enable_addressable_entity {
                    self.create_addressable_entity_from_account(account.clone(), protocol_version)?;
                }

                let footprint = RuntimeFootprint::new_account_footprint(account);
                let entity_addr = EntityAddr::new_account(account_hash.value());
                return Ok((entity_addr, footprint));
            }

            Some(StoredValue::CLValue(contract_key_as_cl_value)) => {
                let key = CLValue::into_t::<Key>(contract_key_as_cl_value)?;
                if let Key::AddressableEntity(addr) = key {
                    addr
                } else {
                    return Err(Self::Error::UnexpectedKeyVariant(key));
                }
            }
            Some(other) => {
                return Err(TrackingCopyError::TypeMismatch(
                    StoredValueTypeMismatch::new("Key".to_string(), other.type_name()),
                ));
            }
            None => return Err(TrackingCopyError::KeyNotFound(account_key)),
        };

        match self.get(&Key::AddressableEntity(entity_addr))? {
            Some(StoredValue::AddressableEntity(entity)) => {
                let named_keys = self.get_named_keys(entity_addr)?;
                let entry_points = self.get_v1_entry_points(entity_addr)?;
                let runtime_footprint = RuntimeFootprint::new_entity_footprint(
                    entity_addr,
                    entity,
                    named_keys,
                    entry_points,
                );
                Ok((entity_addr, runtime_footprint))
            }
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("AddressableEntity".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(Key::AddressableEntity(
                entity_addr,
            ))),
        }
    }

    fn authorized_runtime_footprint_by_account(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        administrative_accounts: &BTreeSet<AccountHash>,
    ) -> Result<(RuntimeFootprint, EntityAddr), Self::Error> {
        let (entity_addr, footprint) =
            self.runtime_footprint_by_account_hash(protocol_version, account_hash)?;

        if !administrative_accounts.is_empty()
            && administrative_accounts
                .intersection(authorization_keys)
                .next()
                .is_some()
        {
            // Exit early if there's at least a single signature coming from an admin.
            return Ok((footprint, entity_addr));
        }

        // Authorize using provided authorization keys
        if !footprint.can_authorize(authorization_keys) {
            return Err(Self::Error::Authorization);
        }

        // Check total key weight against deploy threshold
        if !footprint.can_deploy_with(authorization_keys) {
            return Err(Self::Error::DeploymentAuthorizationFailure);
        }

        Ok((footprint, entity_addr))
    }

    fn authorized_runtime_footprint_with_access_rights(
        &mut self,
        protocol_version: ProtocolVersion,
        initiating_address: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        administrative_accounts: &BTreeSet<AccountHash>,
    ) -> Result<(EntityAddr, RuntimeFootprint, ContextAccessRights), TrackingCopyError> {
        if initiating_address == PublicKey::System.to_account_hash() {
            return self.system_entity_runtime_footprint(protocol_version);
        }

        let (footprint, entity_addr) = self.authorized_runtime_footprint_by_account(
            protocol_version,
            initiating_address,
            authorization_keys,
            administrative_accounts,
        )?;
        let access_rights = footprint.extract_access_rights(entity_addr.value());
        Ok((entity_addr, footprint, access_rights))
    }

    fn system_entity_runtime_footprint(
        &mut self,
        protocol_version: ProtocolVersion,
    ) -> Result<(EntityAddr, RuntimeFootprint, ContextAccessRights), TrackingCopyError> {
        let system_account_hash = PublicKey::System.to_account_hash();
        let (system_entity_addr, mut system_entity) =
            self.runtime_footprint_by_account_hash(protocol_version, system_account_hash)?;

        let system_entity_registry = self.get_system_entity_registry()?;

        let (auction_named_keys, mut auction_access_rights) = {
            let auction_hash = match system_entity_registry.get(AUCTION).copied() {
                Some(auction_hash) => auction_hash,
                None => {
                    error!("unexpected failure; auction not found");
                    return Err(TrackingCopyError::MissingSystemContractHash(
                        AUCTION.to_string(),
                    ));
                }
            };
            let auction = self.runtime_footprint_by_hash_addr(auction_hash)?;
            let auction_access_rights = auction.extract_access_rights(auction_hash);
            (auction.take_named_keys(), auction_access_rights)
        };
        let (mint_named_keys, mint_access_rights) = {
            let mint_hash = match system_entity_registry.get(MINT).copied() {
                Some(mint_hash) => mint_hash,
                None => {
                    error!("unexpected failure; mint not found");
                    return Err(TrackingCopyError::MissingSystemContractHash(
                        MINT.to_string(),
                    ));
                }
            };
            let mint = self.runtime_footprint_by_hash_addr(mint_hash)?;
            let mint_access_rights = mint.extract_access_rights(mint_hash);
            (mint.take_named_keys(), mint_access_rights)
        };

        let (payment_named_keys, payment_access_rights) = {
            let payment_hash = match system_entity_registry.get(HANDLE_PAYMENT).copied() {
                Some(payment_hash) => payment_hash,
                None => {
                    error!("unexpected failure; handle payment not found");
                    return Err(TrackingCopyError::MissingSystemContractHash(
                        HANDLE_PAYMENT.to_string(),
                    ));
                }
            };
            let payment = self.runtime_footprint_by_hash_addr(payment_hash)?;
            let payment_access_rights = payment.extract_access_rights(payment_hash);
            (payment.take_named_keys(), payment_access_rights)
        };

        // the auction calls the mint for total supply behavior, so extending the context to include
        // mint named keys & access rights
        system_entity.named_keys_mut().append(auction_named_keys);
        system_entity.named_keys_mut().append(mint_named_keys);
        system_entity.named_keys_mut().append(payment_named_keys);

        auction_access_rights.extend_access_rights(mint_access_rights.take_access_rights());
        auction_access_rights.extend_access_rights(payment_access_rights.take_access_rights());

        Ok((system_entity_addr, system_entity, auction_access_rights))
    }

    fn migrate_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        named_keys: NamedKeys,
    ) -> Result<(), Self::Error> {
        if !self.enable_addressable_entity {
            return Err(Self::Error::AddressableEntityDisable);
        }

        for (string, key) in named_keys.into_inner().into_iter() {
            let entry_addr = NamedKeyAddr::new_from_string(entity_addr, string.clone())?;
            let named_key_value =
                StoredValue::NamedKey(NamedKeyValue::from_concrete_values(key, string.clone())?);
            let entry_key = Key::NamedKey(entry_addr);
            self.write(entry_key, named_key_value)
        }

        Ok(())
    }

    fn migrate_entry_points(
        &mut self,
        entity_addr: EntityAddr,
        entry_points: EntryPoints,
    ) -> Result<(), Self::Error> {
        if !self.enable_addressable_entity {
            return Err(Self::Error::AddressableEntityDisable);
        }

        if entry_points.is_empty() {
            return Ok(());
        }
        for entry_point in entry_points.take_entry_points().into_iter() {
            let entry_point_addr =
                EntryPointAddr::new_v1_entry_point_addr(entity_addr, entry_point.name())?;
            let entry_point_value =
                StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point));
            self.write(Key::EntryPoint(entry_point_addr), entry_point_value)
        }

        Ok(())
    }

    fn upsert_uref_to_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        name: &str,
        named_keys: &NamedKeys,
        uref: URef,
        stored_value: StoredValue,
    ) -> Result<(), Self::Error> {
        match named_keys.get(name) {
            Some(key) => {
                if let Key::URef(_) = key {
                    self.write(*key, stored_value);
                } else {
                    return Err(Self::Error::UnexpectedKeyVariant(*key));
                }
            }
            None => {
                let uref_key = Key::URef(uref).normalize();
                self.write(uref_key, stored_value);

                if self.enable_addressable_entity {
                    let entry_value = {
                        let named_key_value =
                            NamedKeyValue::from_concrete_values(uref_key, name.to_string())
                                .map_err(Self::Error::CLValue)?;
                        StoredValue::NamedKey(named_key_value)
                    };
                    let entry_key = {
                        let named_key_entry =
                            NamedKeyAddr::new_from_string(entity_addr, name.to_string())
                                .map_err(Self::Error::BytesRepr)?;
                        Key::NamedKey(named_key_entry)
                    };

                    self.write(entry_key, entry_value);
                } else {
                    let named_key_value = StoredValue::CLValue(CLValue::from_t((name, uref_key))?);
                    let base_key = match entity_addr {
                        EntityAddr::System(hash_addr) | EntityAddr::SmartContract(hash_addr) => {
                            Key::Hash(hash_addr)
                        }
                        EntityAddr::Account(addr) => Key::Account(AccountHash::new(addr)),
                    };
                    self.add(base_key, named_key_value)?;
                }
            }
        };
        Ok(())
    }

    fn migrate_account(
        &mut self,
        account_hash: AccountHash,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
        if !self.enable_addressable_entity {
            debug!("ae is not enabled, skipping migration");
            return Ok(());
        }
        let key = Key::Account(account_hash);
        let maybe_stored_value = self.read(&key)?;

        match maybe_stored_value {
            Some(StoredValue::Account(account)) => {
                self.create_addressable_entity_from_account(account, protocol_version)
            }
            Some(StoredValue::CLValue(_)) => Ok(()),
            // This means the Account does not exist, which we consider to be
            // an authorization error. As used by the node, this type of deploy
            // will have already been filtered out, but for other EE use cases
            // and testing it is reachable.
            Some(_) => Err(Self::Error::UnexpectedStoredValueVariant),
            None => Err(Self::Error::AccountNotFound(key)),
        }
    }

    fn create_new_addressable_entity_on_transfer(
        &mut self,
        account_hash: AccountHash,
        main_purse: URef,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
        let mut generator = AddressGenerator::new(main_purse.addr().as_ref(), Phase::System);

        let byte_code_hash = ByteCodeHash::default();
        let entity_hash = AddressableEntityHash::new(account_hash.value());
        let package_hash = PackageHash::new(generator.new_hash_address());

        let associated_keys = AssociatedKeys::new(account_hash, Weight::new(1));

        let action_thresholds: ActionThresholds = Default::default();

        let entity = AddressableEntity::new(
            package_hash,
            byte_code_hash,
            protocol_version,
            main_purse,
            associated_keys,
            action_thresholds,
            EntityKind::Account(account_hash),
        );

        let package = {
            let mut package = Package::new(
                EntityVersions::default(),
                BTreeSet::default(),
                Groups::default(),
                PackageStatus::Locked,
            );
            package.insert_entity_version(protocol_version.value().major, entity_hash);
            package
        };

        let entity_addr = EntityAddr::new_account(entity_hash.value());
        let entity_key = Key::AddressableEntity(entity_addr);

        self.write(entity_key, entity.into());
        self.write(package_hash.into(), package.into());
        let contract_by_account = match CLValue::from_t(entity_key) {
            Ok(cl_value) => cl_value,
            Err(err) => return Err(Self::Error::CLValue(err)),
        };

        self.write(
            Key::Account(account_hash),
            StoredValue::CLValue(contract_by_account),
        );
        Ok(())
    }

    fn create_addressable_entity_from_account(
        &mut self,
        account: Account,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
        let account_hash = account.account_hash();
        if !self.enable_addressable_entity {
            self.write(Key::Account(account_hash), StoredValue::Account(account));
            return Ok(());
        }

        // carry forward the account hash to allow reverse lookup
        let entity_hash = AddressableEntityHash::new(account_hash.value());
        let entity_addr = EntityAddr::new_account(entity_hash.value());

        // migrate named keys -- if this fails there is no reason to proceed further.
        let named_keys = account.named_keys().clone();
        self.migrate_named_keys(entity_addr, named_keys)?;

        // write package first
        let package_hash = {
            let mut generator =
                AddressGenerator::new(account.main_purse().addr().as_ref(), Phase::System);

            let package_hash = PackageHash::new(generator.new_hash_address());

            let mut package = Package::new(
                EntityVersions::default(),
                BTreeSet::default(),
                Groups::default(),
                PackageStatus::Locked,
            );
            package.insert_entity_version(protocol_version.value().major, entity_hash);
            self.write(package_hash.into(), package.into());
            package_hash
        };

        // write entity after package
        {
            // currently, addressable entities of account kind are not permitted to have bytecode
            // however, we intend to revisit this and potentially allow it in a future release
            // as a replacement for stored session.
            let byte_code_hash = ByteCodeHash::default();

            let action_thresholds = {
                let account_threshold = account.action_thresholds().clone();
                ActionThresholds::new(
                    Weight::new(account_threshold.deployment.value()),
                    Weight::new(1u8),
                    Weight::new(account_threshold.key_management.value()),
                )
                .map_err(Self::Error::SetThresholdFailure)?
            };

            let associated_keys = AssociatedKeys::from(account.associated_keys().clone());

            let entity = AddressableEntity::new(
                package_hash,
                byte_code_hash,
                protocol_version,
                account.main_purse(),
                associated_keys,
                action_thresholds,
                EntityKind::Account(account_hash),
            );
            let entity_key = entity.entity_key(entity_hash);
            let contract_by_account = match CLValue::from_t(entity_key) {
                Ok(cl_value) => cl_value,
                Err(err) => return Err(Self::Error::CLValue(err)),
            };

            self.write(entity_key, entity.into());
            self.write(
                Key::Account(account_hash),
                StoredValue::CLValue(contract_by_account),
            );
        }

        Ok(())
    }

    fn migrate_package(
        &mut self,
        legacy_package_key: Key,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
        if !self.enable_addressable_entity {
            return Err(Self::Error::AddressableEntityDisable);
        }

        let legacy_package = match self.read(&legacy_package_key)? {
            Some(StoredValue::ContractPackage(legacy_package)) => legacy_package,
            Some(_) | None => {
                return Err(Self::Error::ValueNotFound(format!(
                    "contract package not found {}",
                    legacy_package_key
                )));
            }
        };

        let legacy_versions = legacy_package.versions().clone();
        let access_uref = legacy_package.access_key();
        let mut generator = AddressGenerator::new(access_uref.addr().as_ref(), Phase::System);

        let package: Package = legacy_package.into();

        for (_, contract_hash) in legacy_versions.into_iter() {
            let contract = match self.read(&Key::Hash(contract_hash.value()))? {
                Some(StoredValue::Contract(legacy_contract)) => legacy_contract,
                Some(_) | None => {
                    return Err(Self::Error::ValueNotFound(format!(
                        "contract not found {}",
                        contract_hash
                    )));
                }
            };

            let purse = generator.new_uref(AccessRights::all());
            let cl_value: CLValue = CLValue::from_t(()).map_err(Self::Error::CLValue)?;
            self.write(Key::URef(purse), StoredValue::CLValue(cl_value));

            let balance_value: CLValue =
                CLValue::from_t(U512::zero()).map_err(Self::Error::CLValue)?;
            self.write(
                Key::Balance(purse.addr()),
                StoredValue::CLValue(balance_value),
            );

            let contract_addr = EntityAddr::new_smart_contract(contract_hash.value());

            let contract_wasm_hash = contract.contract_wasm_hash();

            let updated_entity = AddressableEntity::new(
                PackageHash::new(contract.contract_package_hash().value()),
                ByteCodeHash::new(contract_wasm_hash.value()),
                protocol_version,
                purse,
                AssociatedKeys::default(),
                ActionThresholds::default(),
                EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1),
            );

            let entry_points = contract.entry_points().clone();
            let named_keys = contract.take_named_keys();

            self.migrate_named_keys(contract_addr, named_keys)?;
            self.migrate_entry_points(contract_addr, entry_points.into())?;

            let maybe_previous_wasm = self
                .read(&Key::Hash(contract_wasm_hash.value()))?
                .and_then(|stored_value| stored_value.into_contract_wasm());

            match maybe_previous_wasm {
                None => {
                    return Err(Self::Error::ValueNotFound(format!(
                        "{}",
                        contract_wasm_hash
                    )));
                }
                Some(contract_wasm) => {
                    let byte_code_key = Key::byte_code_key(ByteCodeAddr::new_wasm_addr(
                        updated_entity.byte_code_addr(),
                    ));
                    let byte_code_cl_value = match CLValue::from_t(byte_code_key) {
                        Ok(cl_value) => cl_value,
                        Err(err) => return Err(Self::Error::CLValue(err)),
                    };
                    self.write(
                        Key::Hash(updated_entity.byte_code_addr()),
                        StoredValue::CLValue(byte_code_cl_value),
                    );

                    let byte_code: ByteCode = contract_wasm.into();
                    self.write(byte_code_key, StoredValue::ByteCode(byte_code));
                }
            }

            let entity_hash = AddressableEntityHash::new(contract_hash.value());
            let entity_key = Key::contract_entity_key(entity_hash);
            let indirection = match CLValue::from_t(entity_key) {
                Ok(cl_value) => cl_value,
                Err(err) => return Err(Self::Error::CLValue(err)),
            };
            self.write(
                Key::Hash(contract_hash.value()),
                StoredValue::CLValue(indirection),
            );

            self.write(entity_key, StoredValue::AddressableEntity(updated_entity));
        }

        let package_key = Key::SmartContract(
            legacy_package_key
                .into_hash_addr()
                .ok_or(Self::Error::UnexpectedKeyVariant(legacy_package_key))?,
        );

        let access_key_value =
            CLValue::from_t((package_key, access_uref)).map_err(Self::Error::CLValue)?;
        self.write(legacy_package_key, StoredValue::CLValue(access_key_value));
        self.write(package_key, StoredValue::SmartContract(package));
        Ok(())
    }

    fn fees_purse(
        &mut self,
        protocol_version: ProtocolVersion,
        fees_purse_handling: FeesPurseHandling,
    ) -> Result<URef, TrackingCopyError> {
        let fee_handling = fees_purse_handling;
        match fee_handling {
            FeesPurseHandling::None(uref) => Ok(uref),
            FeesPurseHandling::ToProposer(proposer) => {
                let (_, entity) =
                    self.runtime_footprint_by_account_hash(protocol_version, proposer)?;
                Ok(entity
                    .main_purse()
                    .ok_or(TrackingCopyError::AddressableEntityDisable)?)
            }
            FeesPurseHandling::Accumulate => {
                let registry = self.get_system_entity_registry()?;
                let entity_addr = {
                    let hash = match registry.get(HANDLE_PAYMENT) {
                        Some(hash) => hash,
                        None => {
                            return Err(TrackingCopyError::MissingSystemContractHash(
                                HANDLE_PAYMENT.to_string(),
                            ));
                        }
                    };
                    EntityAddr::new_system(*hash)
                };

                let named_keys = self.get_named_keys(entity_addr)?;

                let accumulation_purse_uref = match named_keys.get(ACCUMULATION_PURSE_KEY) {
                    Some(Key::URef(accumulation_purse)) => *accumulation_purse,
                    Some(_) | None => {
                        error!(
                            "fee handling is configured to accumulate but handle payment does not \
                            have accumulation purse"
                        );
                        return Err(TrackingCopyError::NamedKeyNotFound(
                            ACCUMULATION_PURSE_KEY.to_string(),
                        ));
                    }
                };

                Ok(accumulation_purse_uref)
            }
            FeesPurseHandling::Burn(uref) => Ok(uref),
        }
    }

    fn system_contract_named_key(
        &mut self,
        system_contract_name: &str,
        name: &str,
    ) -> Result<Option<Key>, Self::Error> {
        let system_entity_registry = self.get_system_entity_registry()?;
        let hash = match system_entity_registry.get(system_contract_name).copied() {
            Some(hash) => hash,
            None => {
                error!(
                    "unexpected failure; system contract {} not found",
                    system_contract_name
                );
                return Err(TrackingCopyError::MissingSystemContractHash(
                    system_contract_name.to_string(),
                ));
            }
        };
        let runtime_footprint = self.runtime_footprint_by_hash_addr(hash)?;
        Ok(runtime_footprint.take_named_keys().get(name).copied())
    }
}
