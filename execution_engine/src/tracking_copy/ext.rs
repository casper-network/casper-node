use std::{
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
};

use casper_storage::global_state::{state::StateReader, trie::merkle_proof::TrieMerkleProof};
use casper_types::{
    account::AccountHash,
    addressable_entity::{EntityKindTag, NamedKeyAddr, NamedKeys},
    bytesrepr,
    package::{EntityVersions, Groups, PackageStatus},
    AccessRights, AddressableEntity, AddressableEntityHash, CLValue, EntityAddr, EntityKind,
    EntryPoints, Key, Motes, Package, PackageHash, Phase, ProtocolVersion, StoredValue,
    StoredValueTypeMismatch, URef, KEY_HASH_LENGTH,
};

use crate::{
    engine_state::{ChecksumRegistry, SystemContractRegistry, ACCOUNT_BYTE_CODE_HASH},
    execution,
    execution::AddressGenerator,
    tracking_copy::TrackingCopy,
};

/// Higher-level operations on the state via a `TrackingCopy`.
pub trait TrackingCopyExt<R> {
    /// The type for the returned errors.
    type Error;

    /// Gets the contract hash for the account at a given account address.
    fn get_entity_hash_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<AddressableEntityHash, Self::Error>;

    /// Gets the entity for a given account by its account address
    fn get_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error>;

    /// Reads the entity key for the account at a given account address.
    fn read_account(&mut self, account_hash: AccountHash) -> Result<Key, Self::Error>;

    /// Reads the entity for a given account by its account address
    fn read_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error>;

    /// Gets the purse balance key for a given purse id.
    fn get_purse_balance_key(&self, purse_key: Key) -> Result<Key, Self::Error>;

    /// Gets the balance at a given balance key.
    fn get_purse_balance(&self, balance_key: Key) -> Result<Motes, Self::Error>;

    /// Gets the purse balance key for a given purse id and provides a Merkle proof.
    fn get_purse_balance_key_with_proof(
        &self,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Gets the balance at a given balance key and provides a Merkle proof.
    fn get_purse_balance_with_proof(
        &self,
        balance_key: Key,
    ) -> Result<(Motes, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Gets an addressable entity  by Key.
    fn get_contract(
        &mut self,
        contract_hash: AddressableEntityHash,
    ) -> Result<AddressableEntity, Self::Error>;
    fn get_named_keys(&mut self, entity_addr: EntityAddr) -> Result<NamedKeys, Self::Error>;

    /// Gets a package by Key.
    fn get_package(&mut self, contract_package_hash: PackageHash) -> Result<Package, Self::Error>;

    /// Gets an entity by Key.
    fn get_contract_entity(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<(AddressableEntity, bool), Self::Error>;

    /// Gets the system contract registry.
    fn get_system_contracts(&mut self) -> Result<SystemContractRegistry, Self::Error>;

    /// Gets the system checksum registry.
    fn get_checksum_registry(&mut self) -> Result<Option<ChecksumRegistry>, Self::Error>;

    fn migrate_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        named_keys: NamedKeys,
    ) -> Result<(), Self::Error>;
}

impl<R> TrackingCopyExt<R> for TrackingCopy<R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = execution::Error;

    fn get_entity_hash_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<AddressableEntityHash, Self::Error> {
        let account_key = Key::Account(account_hash);
        match self.get(&account_key).map_err(Into::into)? {
            Some(StoredValue::CLValue(cl_value)) => {
                let entity_key = CLValue::into_t::<Key>(cl_value)?;
                let entity_hash = AddressableEntityHash::try_from(entity_key)
                    .map_err(|_| execution::Error::BytesRepr(bytesrepr::Error::Formatting))?;

                Ok(entity_hash)
            }
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(account_key)),
        }
    }

    fn get_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error> {
        let account_key = Key::Account(account_hash);

        let contract_key = match self.get(&account_key).map_err(Into::into)? {
            Some(StoredValue::CLValue(contract_key_as_cl_value)) => {
                CLValue::into_t::<Key>(contract_key_as_cl_value)?
            }
            Some(StoredValue::Account(account)) => {
                let mut generator =
                    AddressGenerator::new(account.main_purse().addr().as_ref(), Phase::System);

                let contract_wasm_hash = *ACCOUNT_BYTE_CODE_HASH;
                let entity_hash = AddressableEntityHash::new(generator.new_hash_address());
                let package_hash = PackageHash::new(generator.new_hash_address());

                let entry_points = EntryPoints::new();

                self.migrate_named_keys(
                    EntityAddr::Account(entity_hash.value()),
                    account.named_keys().clone(),
                )?;

                let entity = AddressableEntity::new(
                    package_hash,
                    contract_wasm_hash,
                    entry_points,
                    protocol_version,
                    account.main_purse(),
                    account.associated_keys().clone().into(),
                    account.action_thresholds().clone().into(),
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
                    Err(error) => return Err(execution::Error::CLValue(error)),
                };

                self.write(account_key, StoredValue::CLValue(contract_by_account));

                return Ok(entity);
            }

            Some(other) => {
                return Err(execution::Error::TypeMismatch(
                    StoredValueTypeMismatch::new("Key".to_string(), other.type_name()),
                ));
            }
            None => return Err(execution::Error::KeyNotFound(account_key)),
        };

        match self.get(&contract_key).map_err(Into::into)? {
            Some(StoredValue::AddressableEntity(contract)) => Ok(contract),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("Contract".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(contract_key)),
        }
    }

    fn read_account(&mut self, account_hash: AccountHash) -> Result<Key, Self::Error> {
        let account_key = Key::Account(account_hash);
        match self.read(&account_key).map_err(Into::into)? {
            Some(StoredValue::CLValue(cl_value)) => Ok(CLValue::into_t(cl_value)?),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("Account".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(account_key)),
        }
    }

    fn read_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error> {
        self.get_addressable_entity_by_account_hash(protocol_version, account_hash)
    }

    fn get_purse_balance_key(&self, purse_key: Key) -> Result<Key, Self::Error> {
        let balance_key: URef = purse_key
            .into_uref()
            .ok_or(execution::Error::KeyIsNotAURef(purse_key))?;
        Ok(Key::Balance(balance_key.addr()))
    }

    fn get_purse_balance(&self, key: Key) -> Result<Motes, Self::Error> {
        let stored_value: StoredValue = self
            .read(&key)
            .map_err(Into::into)?
            .ok_or(execution::Error::KeyNotFound(key))?;
        let cl_value: CLValue = stored_value
            .try_into()
            .map_err(execution::Error::TypeMismatch)?;
        let balance = Motes::new(cl_value.into_t()?);
        Ok(balance)
    }

    fn get_purse_balance_key_with_proof(
        &self,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        let balance_key: Key = purse_key
            .uref_to_hash()
            .ok_or(execution::Error::KeyIsNotAURef(purse_key))?;
        let proof: TrieMerkleProof<Key, StoredValue> = self
            .read_with_proof(&balance_key) // Key::Hash, so no need to normalize
            .map_err(Into::into)?
            .ok_or(execution::Error::KeyNotFound(purse_key))?;
        let stored_value_ref: &StoredValue = proof.value();
        let cl_value: CLValue = stored_value_ref
            .to_owned()
            .try_into()
            .map_err(execution::Error::TypeMismatch)?;
        let balance_key: Key = cl_value.into_t()?;
        Ok((balance_key, proof))
    }

    fn get_purse_balance_with_proof(
        &self,
        key: Key,
    ) -> Result<(Motes, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        let proof: TrieMerkleProof<Key, StoredValue> = self
            .read_with_proof(&key.normalize())
            .map_err(Into::into)?
            .ok_or(execution::Error::KeyNotFound(key))?;
        let cl_value: CLValue = proof
            .value()
            .to_owned()
            .try_into()
            .map_err(execution::Error::TypeMismatch)?;
        let balance = Motes::new(cl_value.into_t()?);
        Ok((balance, proof))
    }

    /// Gets a contract header by Key
    fn get_contract(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<AddressableEntity, Self::Error> {
        let package_kind_tag = if self.get_system_contracts()?.has_contract_hash(&entity_hash) {
            EntityKindTag::System
        } else {
            EntityKindTag::SmartContract
        };

        let key = Key::addressable_entity_key(package_kind_tag, entity_hash);

        match self.read(&key).map_err(Into::into)? {
            Some(StoredValue::AddressableEntity(entity)) => Ok(entity),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new(
                    "AddressableEntity or Contract".to_string(),
                    other.type_name(),
                ),
            )),
            None => Err(execution::Error::KeyNotFound(key)),
        }
    }

    fn migrate_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        named_keys: NamedKeys,
    ) -> Result<(), Self::Error> {
        let base_named_key = NamedKeyAddr::new_named_key_base(entity_addr);
        self.write(
            Key::NamedKey(base_named_key),
            StoredValue::CLValue(CLValue::unit()),
        );

        for (name, key) in named_keys.iter() {
            let entry_key = {
                let entry_addr = NamedKeyAddr::new_from_string(entity_addr, name.clone())?;
                Key::NamedKey(entry_addr)
            };

            let cl_value =
                CLValue::from_t(*key).map_err(|cl_error| Self::Error::CLValue(cl_error.clone()))?;

            self.write(entry_key, StoredValue::CLValue(cl_value))
        }

        Ok(())
    }

    fn get_named_keys(&mut self, entity_addr: EntityAddr) -> Result<NamedKeys, Self::Error> {
        let base_named_key_addr = NamedKeyAddr::Base(entity_addr);

        let prefix = base_named_key_addr
            .named_keys_prefix()
            .map_err(|error| Self::Error::BytesRepr(error))?;

        let entries = self.reader.keys_with_prefix(&prefix).map_err(Into::into)?;

        let mut named_keys = NamedKeys::new();

        for entry_key in entries.iter() {
            match self.read(entry_key).map_err(Into::into)? {
                Some(StoredValue::NamedKey(named_key)) => {
                    let key = named_key.get_key()
                        .map_err(|cl_error| execution::Error::CLValue(cl_error.clone()))?;
                    let name = named_key.get_name()
                        .map_err(|cl_error| execution::Error::CLValue(cl_error.clone()))?;
                    named_keys.insert(name, key);
                }
                Some(other) => {
                    return Err(execution::Error::TypeMismatch(
                        StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
                    ))
                }
                None => return Err(execution::Error::KeyNotFound(*entry_key)),
            };
        }

        Ok(named_keys)
    }

    fn get_package(&mut self, package_hash: PackageHash) -> Result<Package, Self::Error> {
        let key = package_hash.into();
        match self.read(&key).map_err(Into::into)? {
            Some(StoredValue::Package(contract_package)) => Ok(contract_package),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("Package".to_string(), other.type_name()),
            )),
            None => match self
                .read(&Key::Hash(package_hash.value()))
                .map_err(Into::into)?
            {
                Some(StoredValue::ContractPackage(contract_package)) => {
                    let package: Package = contract_package.into();
                    self.write(
                        Key::Package(package_hash.value()),
                        StoredValue::Package(package.clone()),
                    );
                    Ok(package)
                }
                Some(other) => Err(execution::Error::TypeMismatch(
                    StoredValueTypeMismatch::new("ContractPackage".to_string(), other.type_name()),
                )),
                None => Err(execution::Error::KeyNotFound(key)),
            },
        }
    }

    fn get_contract_entity(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<(AddressableEntity, bool), Self::Error> {
        let key = Key::contract_entity_key(entity_hash);
        match self.read(&key).map_err(Into::into)? {
            Some(StoredValue::AddressableEntity(entity)) => Ok((entity, false)),
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("AddressableEntity".to_string(), other.type_name()),
            )),
            None => match self
                .read(&Key::Hash(entity_hash.value()))
                .map_err(Into::into)?
            {
                Some(StoredValue::Contract(contract)) => Ok((contract.into(), true)),
                Some(other) => Err(execution::Error::TypeMismatch(
                    StoredValueTypeMismatch::new("Contract".to_string(), other.type_name()),
                )),
                None => Err(execution::Error::KeyNotFound(key)),
            },
        }
    }

    fn get_system_contracts(&mut self) -> Result<SystemContractRegistry, Self::Error> {
        match self.get(&Key::SystemContractRegistry).map_err(Into::into)? {
            Some(StoredValue::CLValue(registry)) => {
                let registry: SystemContractRegistry =
                    CLValue::into_t(registry).map_err(Self::Error::from)?;
                Ok(registry)
            }
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Err(execution::Error::KeyNotFound(Key::SystemContractRegistry)),
        }
    }

    fn get_checksum_registry(&mut self) -> Result<Option<ChecksumRegistry>, Self::Error> {
        match self.get(&Key::ChecksumRegistry).map_err(Into::into)? {
            Some(StoredValue::CLValue(registry)) => {
                let registry: ChecksumRegistry =
                    CLValue::into_t(registry).map_err(Self::Error::from)?;
                Ok(Some(registry))
            }
            Some(other) => Err(execution::Error::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Ok(None),
        }
    }
}
