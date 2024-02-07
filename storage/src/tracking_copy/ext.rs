use std::{
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
};

use crate::{
    global_state::{
        error::Error as GlobalStateError, state::StateReader, trie::merkle_proof::TrieMerkleProof,
    },
    AddressGenerator,
};

use casper_types::{
    account::AccountHash,
    addressable_entity::MessageTopics,
    bytesrepr,
    package::{EntityVersions, Groups, PackageKind, PackageKindTag, PackageStatus},
    AccessRights, AddressableEntity, AddressableEntityHash, ByteCode, ByteCodeHash, ByteCodeKind,
    CLValue, ChecksumRegistry, EntryPoints, Key, Motes, Package, PackageHash, Phase,
    ProtocolVersion, StoredValue, StoredValueTypeMismatch, SystemContractRegistry, URef,
};

use crate::tracking_copy::{TrackingCopy, TrackingCopyError};

/// Higher-level operations on the state via a `TrackingCopy`.
pub trait TrackingCopyExt<R> {
    /// The type for the returned errors.
    type Error;

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

    /// Reads the entity key for a given account hash.
    fn read_account_key(&mut self, account_hash: AccountHash) -> Result<Key, Self::Error>;

    /// Reads the entity by its account hash.
    fn read_addressable_entity_by_account_hash(
        &mut self,
        protocol_version: ProtocolVersion,
        account_hash: AccountHash,
    ) -> Result<AddressableEntity, Self::Error>;

    /// Gets the purse balance key for a given purse.
    fn get_purse_balance_key(&self, purse_key: Key) -> Result<Key, Self::Error>;

    /// Gets the balance for a given balance key.
    fn get_purse_balance(&self, balance_key: Key) -> Result<Motes, Self::Error>;

    /// Gets the purse balance key for a given purse and provides a Merkle proof.
    fn get_purse_balance_key_with_proof(
        &self,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Gets the balance at a given balance key and provides a Merkle proof.
    fn get_purse_balance_with_proof(
        &self,
        balance_key: Key,
    ) -> Result<(Motes, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Gets an addressable entity by hash.
    fn get_addressable_entity(
        &mut self,
        addressable_entity_hash: AddressableEntityHash,
    ) -> Result<AddressableEntity, Self::Error>;

    /// Gets an entity by hash.
    fn get_entity(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<(AddressableEntity, bool), Self::Error>;

    /// Gets a package by hash.
    fn get_package(&mut self, package_hash: PackageHash) -> Result<Package, Self::Error>;

    /// Gets the system contract registry.
    fn get_system_contracts(&mut self) -> Result<SystemContractRegistry, Self::Error>;

    /// Gets the system checksum registry.
    fn get_checksum_registry(&mut self) -> Result<Option<ChecksumRegistry>, Self::Error>;

    /// Gets byte code by hash.
    fn get_byte_code(&mut self, byte_code_hash: ByteCodeHash) -> Result<ByteCode, Self::Error>;
}

impl<R> TrackingCopyExt<R> for TrackingCopy<R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    type Error = TrackingCopyError;

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

                let entity = AddressableEntity::new(
                    package_hash,
                    byte_code_hash,
                    account.named_keys().clone(),
                    entry_points,
                    protocol_version,
                    account.main_purse(),
                    account.associated_keys().clone().into(),
                    account.action_thresholds().clone().into(),
                    MessageTopics::default(),
                );

                let access_key = generator.new_uref(AccessRights::READ_ADD_WRITE);

                let contract_package = {
                    let mut contract_package = Package::new(
                        access_key,
                        EntityVersions::default(),
                        BTreeSet::default(),
                        Groups::default(),
                        PackageStatus::Locked,
                        PackageKind::Account(account_hash),
                    );
                    contract_package
                        .insert_entity_version(protocol_version.value().major, entity_hash);
                    contract_package
                };

                let entity_key = Key::addressable_entity_key(PackageKindTag::Account, entity_hash);

                self.write(entity_key, StoredValue::AddressableEntity(entity.clone()));
                self.write(package_hash.into(), contract_package.into());

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

    fn read_account_key(&mut self, account_hash: AccountHash) -> Result<Key, Self::Error> {
        let account_key = Key::Account(account_hash);
        match self.read(&account_key)? {
            Some(StoredValue::CLValue(cl_value)) => Ok(CLValue::into_t(cl_value)?),
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("Account".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(account_key)),
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
            .ok_or(TrackingCopyError::KeyIsNotAURef(purse_key))?;
        Ok(Key::Balance(balance_key.addr()))
    }

    fn get_purse_balance(&self, key: Key) -> Result<Motes, Self::Error> {
        let stored_value: StoredValue = self
            .read(&key)?
            .ok_or(TrackingCopyError::KeyNotFound(key))?;
        let cl_value: CLValue = stored_value
            .try_into()
            .map_err(TrackingCopyError::TypeMismatch)?;
        let balance = Motes::new(cl_value.into_t()?);
        Ok(balance)
    }

    fn get_purse_balance_key_with_proof(
        &self,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        let balance_key: Key = purse_key
            .uref_to_hash()
            .ok_or(TrackingCopyError::KeyIsNotAURef(purse_key))?;
        let proof: TrieMerkleProof<Key, StoredValue> = self
            .read_with_proof(&balance_key)?
            .ok_or(TrackingCopyError::KeyNotFound(purse_key))?;
        let stored_value_ref: &StoredValue = proof.value();
        let cl_value: CLValue = stored_value_ref
            .to_owned()
            .try_into()
            .map_err(TrackingCopyError::TypeMismatch)?;
        let balance_key: Key = cl_value.into_t()?;
        Ok((balance_key, proof))
    }

    fn get_purse_balance_with_proof(
        &self,
        key: Key,
    ) -> Result<(Motes, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        let proof: TrieMerkleProof<Key, StoredValue> = self
            .read_with_proof(&key.normalize())?
            .ok_or(TrackingCopyError::KeyNotFound(key))?;
        let cl_value: CLValue = proof
            .value()
            .to_owned()
            .try_into()
            .map_err(TrackingCopyError::TypeMismatch)?;
        let balance = Motes::new(cl_value.into_t()?);
        Ok((balance, proof))
    }

    fn get_byte_code(&mut self, byte_code_hash: ByteCodeHash) -> Result<ByteCode, Self::Error> {
        let key = Key::ByteCode(ByteCodeKind::V1CasperWasm, byte_code_hash.value());
        match self.get(&key)? {
            Some(StoredValue::ByteCode(byte_code)) => Ok(byte_code),
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("ContractWasm".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(key)),
        }
    }

    fn get_addressable_entity(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<AddressableEntity, Self::Error> {
        let package_kind_tag = if self.get_system_contracts()?.has_contract_hash(&entity_hash) {
            PackageKindTag::System
        } else {
            PackageKindTag::SmartContract
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

    fn get_package(&mut self, package_hash: PackageHash) -> Result<Package, Self::Error> {
        let key = package_hash.into();
        match self.read(&key)? {
            Some(StoredValue::Package(contract_package)) => Ok(contract_package),
            Some(other) => Err(Self::Error::TypeMismatch(StoredValueTypeMismatch::new(
                "Package".to_string(),
                other.type_name(),
            ))),
            None => match self.read(&Key::Hash(package_hash.value()))? {
                Some(StoredValue::ContractPackage(contract_package)) => {
                    let package: Package = contract_package.into();
                    self.write(
                        Key::Package(package_hash.value()),
                        StoredValue::Package(package.clone()),
                    );
                    Ok(package)
                }
                Some(other) => Err(TrackingCopyError::TypeMismatch(
                    StoredValueTypeMismatch::new("ContractPackage".to_string(), other.type_name()),
                )),
                None => Err(Self::Error::KeyNotFound(key)),
            },
        }
    }

    fn get_entity(
        &mut self,
        entity_hash: AddressableEntityHash,
    ) -> Result<(AddressableEntity, bool), Self::Error> {
        let key = Key::contract_entity_key(entity_hash);
        match self.read(&key)? {
            Some(StoredValue::AddressableEntity(entity)) => Ok((entity, false)),
            Some(other) => Err(Self::Error::TypeMismatch(StoredValueTypeMismatch::new(
                "AddressableEntity".to_string(),
                other.type_name(),
            ))),
            None => match self.read(&Key::Hash(entity_hash.value()))? {
                Some(StoredValue::Contract(contract)) => Ok((contract.into(), true)),
                Some(other) => Err(Self::Error::TypeMismatch(StoredValueTypeMismatch::new(
                    "Contract".to_string(),
                    other.type_name(),
                ))),
                None => Err(TrackingCopyError::KeyNotFound(key)),
            },
        }
    }

    fn get_system_contracts(&mut self) -> Result<SystemContractRegistry, Self::Error> {
        match self.get(&Key::SystemContractRegistry)? {
            Some(StoredValue::CLValue(registry)) => {
                let registry: SystemContractRegistry =
                    CLValue::into_t(registry).map_err(Self::Error::from)?;
                Ok(registry)
            }
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(Key::SystemContractRegistry)),
        }
    }

    fn get_checksum_registry(&mut self) -> Result<Option<ChecksumRegistry>, Self::Error> {
        match self.get(&Key::ChecksumRegistry)? {
            Some(StoredValue::CLValue(registry)) => {
                let registry: ChecksumRegistry =
                    CLValue::into_t(registry).map_err(Self::Error::from)?;
                Ok(Some(registry))
            }
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Ok(None),
        }
    }
}
