use std::{collections::BTreeSet, convert::TryFrom};

use casper_types::{
    account::AccountHash,
    addressable_entity::{
        ActionThresholds, AssociatedKeys, EntityKindTag, MessageTopics, NamedKeyAddr,
        NamedKeyValue, NamedKeys, Weight,
    },
    bytesrepr,
    package::{EntityVersions, Groups, PackageStatus},
    AccessRights, Account, AddressableEntity, AddressableEntityHash, ByteCodeHash, CLValue,
    EntityAddr, EntityKind, EntryPoints, Key, Package, PackageHash, Phase, ProtocolVersion,
    StoredValue, StoredValueTypeMismatch,
};

use crate::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyError, TrackingCopyExt},
    AddressGenerator,
};

/// Higher-level operations on the state via a `TrackingCopy`.
pub trait TrackingCopyEntityExt<R> {
    /// The type for the returned errors.
    type Error;

    /// Gets an addressable entity by hash.
    fn get_addressable_entity(
        &mut self,
        addressable_entity_hash: AddressableEntityHash,
    ) -> Result<AddressableEntity, Self::Error>;

    /// Gets the entity hash for an account hash.
    fn get_entity_hash_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<AddressableEntityHash, Self::Error>;

    /// Gets the entity for a given account by its account hash.
    fn get_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error>;

    /// Reads the entity by its account hash.
    fn read_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error>;

    /// Get entity if authorized, else error.
    fn get_authorized_addressable_entity(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        administrative_accounts: &BTreeSet<AccountHash>,
    ) -> Result<(AddressableEntity, AddressableEntityHash), Self::Error>;

    /// Migrate the NamedKeys for a Contract or Account.
    fn migrate_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        named_keys: NamedKeys,
    ) -> Result<(), Self::Error>;

    fn migrate_account(
        &mut self,
        account_hash: AccountHash,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error>;

    fn create_addressable_entity_from_account(
        &mut self,
        account: Account,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error>;
}

impl<R> TrackingCopyEntityExt<R> for TrackingCopy<R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    type Error = TrackingCopyError;

    fn get_addressable_entity(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<AddressableEntity, Self::Error> {
        let package_kind_tag = if self
            .get_system_entity_registry()?
            .has_contract_hash(&entity_hash)
        {
            EntityKindTag::System
        } else {
            EntityKindTag::SmartContract
        };

        let key = Key::addressable_entity_key(package_kind_tag, entity_hash);

        match self.read(&key)? {
            Some(StoredValue::AddressableEntity(entity)) => Ok(entity),
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new(
                    "AddressableEntity or Contract".to_string(),
                    other.type_name(),
                ),
            )),
            None => Err(TrackingCopyError::KeyNotFound(key)),
        }
    }

    fn get_entity_hash_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<AddressableEntityHash, Self::Error> {
        let account_key = Key::Account(account_hash);
        match self.get(&account_key)? {
            Some(StoredValue::CLValue(cl_value)) => {
                let entity_key = CLValue::into_t::<Key>(cl_value)?;
                let entity_hash = AddressableEntityHash::try_from(entity_key)
                    .map_err(|_| TrackingCopyError::BytesRepr(bytesrepr::Error::Formatting))?;

                Ok(entity_hash)
            }
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(account_key)),
        }
    }

    fn get_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error> {
        let account_key = Key::Account(account_hash);

        let contract_key = match self.get(&account_key)? {
            Some(StoredValue::CLValue(contract_key_as_cl_value)) => {
                CLValue::into_t::<Key>(contract_key_as_cl_value)?
            }
            Some(StoredValue::Account(account)) => {
                // do a legacy account migration
                let mut generator =
                    AddressGenerator::new(account.main_purse().addr().as_ref(), Phase::System);

                let byte_code_hash = ByteCodeHash::default();
                let entity_hash = AddressableEntityHash::new(generator.new_hash_address());
                let package_hash = PackageHash::new(generator.new_hash_address());

                let entry_points = EntryPoints::new();

                self.migrate_named_keys(
                    EntityAddr::Account(entity_hash.value()),
                    account.named_keys().clone(),
                )?;

                let entity = AddressableEntity::new(
                    package_hash,
                    byte_code_hash,
                    entry_points,
                    protocol_version,
                    account.main_purse(),
                    account.associated_keys().clone().into(),
                    account.action_thresholds().clone().into(),
                    MessageTopics::default(),
                    EntityKind::Account(account_hash),
                );

                let access_key = generator.new_uref(AccessRights::READ_ADD_WRITE);

                let package = {
                    let mut package = Package::new(
                        access_key,
                        EntityVersions::default(),
                        BTreeSet::default(),
                        Groups::default(),
                        PackageStatus::Locked,
                    );
                    package.insert_entity_version(protocol_version.value().major, entity_hash);
                    package
                };

                let entity_key = entity.entity_key(entity_hash);

                self.write(entity_key, StoredValue::AddressableEntity(entity.clone()));
                self.write(package_hash.into(), package.into());

                let contract_by_account = match CLValue::from_t(entity_key) {
                    Ok(cl_value) => cl_value,
                    Err(error) => return Err(TrackingCopyError::CLValue(error)),
                };

                self.write(account_key, StoredValue::CLValue(contract_by_account));

                return Ok(entity);
            }

            Some(other) => {
                return Err(TrackingCopyError::TypeMismatch(
                    StoredValueTypeMismatch::new("Key".to_string(), other.type_name()),
                ));
            }
            None => return Err(TrackingCopyError::KeyNotFound(account_key)),
        };

        match self.get(&contract_key)? {
            Some(StoredValue::AddressableEntity(contract)) => Ok(contract),
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("Contract".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(contract_key)),
        }
    }

    fn read_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error> {
        self.get_addressable_entity_by_account_hash(protocol_version, account_hash)
    }

    fn get_authorized_addressable_entity(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        administrative_accounts: &BTreeSet<AccountHash>,
    ) -> Result<(AddressableEntity, AddressableEntityHash), Self::Error> {
        let entity_record =
            self.get_addressable_entity_by_account_hash(protocol_version, account_hash)?;

        let entity_hash = self.get_entity_hash_by_account_hash(account_hash)?;

        if !administrative_accounts.is_empty()
            && administrative_accounts
                .intersection(authorization_keys)
                .next()
                .is_some()
        {
            // Exit early if there's at least a single signature coming from an admin.
            return Ok((entity_record, entity_hash));
        }

        // Authorize using provided authorization keys
        if !entity_record.can_authorize(authorization_keys) {
            return Err(Self::Error::Authorization);
        }

        // Check total key weight against deploy threshold
        if !entity_record.can_deploy_with(authorization_keys) {
            return Err(Self::Error::DeploymentAuthorizationFailure);
        }

        Ok((entity_record, entity_hash))
    }

    fn migrate_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        named_keys: NamedKeys,
    ) -> Result<(), Self::Error> {
        for (string, key) in named_keys.into_inner().into_iter() {
            let entry_addr = NamedKeyAddr::new_from_string(entity_addr, string.clone())?;

            let named_key_value =
                StoredValue::NamedKey(NamedKeyValue::from_concrete_values(key, string.clone())?);

            let entry_key = Key::NamedKey(entry_addr);

            self.write(entry_key, named_key_value)
        }

        Ok(())
    }

    fn migrate_account(
        &mut self,
        account_hash: AccountHash,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
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

    fn create_addressable_entity_from_account(
        &mut self,
        account: Account,
        protocol_version: ProtocolVersion,
    ) -> Result<(), Self::Error> {
        let account_hash = account.account_hash();

        let mut generator =
            AddressGenerator::new(account.main_purse().addr().as_ref(), Phase::System);

        let byte_code_hash = ByteCodeHash::default();
        let entity_hash = AddressableEntityHash::new(generator.new_hash_address());
        let package_hash = PackageHash::new(generator.new_hash_address());

        let entry_points = EntryPoints::new();

        let associated_keys = AssociatedKeys::from(account.associated_keys().clone());
        let action_thresholds = {
            let account_threshold = account.action_thresholds().clone();
            ActionThresholds::new(
                Weight::new(account_threshold.deployment.value()),
                Weight::new(1u8),
                Weight::new(account_threshold.key_management.value()),
            )
            .map_err(Self::Error::SetThresholdFailure)?
        };

        let entity_addr = EntityAddr::new_account_entity_addr(entity_hash.value());

        self.migrate_named_keys(entity_addr, account.named_keys().clone())?;

        let entity = AddressableEntity::new(
            package_hash,
            byte_code_hash,
            entry_points,
            protocol_version,
            account.main_purse(),
            associated_keys,
            action_thresholds,
            MessageTopics::default(),
            EntityKind::Account(account_hash),
        );

        let access_key = generator.new_uref(AccessRights::READ_ADD_WRITE);

        let package = {
            let mut package = Package::new(
                access_key,
                EntityVersions::default(),
                BTreeSet::default(),
                Groups::default(),
                PackageStatus::Locked,
            );
            package.insert_entity_version(protocol_version.value().major, entity_hash);
            package
        };

        let entity_key: Key = entity.entity_key(entity_hash);

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
}
