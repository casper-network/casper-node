use std::{collections::BTreeSet, convert::TryInto};

use crate::global_state::{error::Error as GlobalStateError, state::StateReader};

use casper_types::{
    account::AccountHash, addressable_entity::NamedKeys, global_state::TrieMerkleProof, ByteCode,
    ByteCodeAddr, ByteCodeHash, CLValue, ChecksumRegistry, EntityAddr, Key, KeyTag, Motes, Package,
    PackageHash, StoredValue, StoredValueTypeMismatch, SystemEntityRegistry, URef,
};

use crate::tracking_copy::{TrackingCopy, TrackingCopyError};

/// Higher-level operations on the state via a `TrackingCopy`.
pub trait TrackingCopyExt<R> {
    /// The type for the returned errors.
    type Error;

    /// Reads the entity key for a given account hash.
    fn read_account_key(&mut self, account_hash: AccountHash) -> Result<Key, Self::Error>;

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

    /// Returns the collection of named keys for a given AddressableEntity.
    fn get_named_keys(&mut self, entity_addr: EntityAddr) -> Result<NamedKeys, Self::Error>;

    /// Gets a package by hash.
    fn get_package(&mut self, package_hash: PackageHash) -> Result<Package, Self::Error>;

    /// Gets the system contract registry.
    fn get_system_entity_registry(&mut self) -> Result<SystemEntityRegistry, Self::Error>;

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
        let key = Key::ByteCode(ByteCodeAddr::V1CasperWasm(byte_code_hash.value()));
        match self.get(&key)? {
            Some(StoredValue::ByteCode(byte_code)) => Ok(byte_code),
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("ContractWasm".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(key)),
        }
    }

    fn get_named_keys(&mut self, entity_addr: EntityAddr) -> Result<NamedKeys, Self::Error> {
        let prefix = entity_addr
            .named_keys_prefix()
            .map_err(Self::Error::BytesRepr)?;

        let mut ret: BTreeSet<Key> = BTreeSet::new();
        let keys = self.reader.keys_with_prefix(&prefix)?;
        let pruned = &self.cache.prunes_cached;
        // don't include keys marked for pruning
        for key in keys {
            if pruned.contains(&key) {
                continue;
            }
            ret.insert(key);
        }

        let cache = self.cache.get_key_tag_muts_cached(&KeyTag::NamedKey);

        // there may be newly inserted keys which have not been committed yet
        if let Some(keys) = cache {
            for key in keys {
                if ret.contains(&key) {
                    continue;
                }
                if key.is_entry_for_base(&entity_addr) {
                    ret.insert(key);
                }
            }
        }

        let mut named_keys = NamedKeys::new();

        for entry_key in ret.iter() {
            match self.read(entry_key)? {
                Some(StoredValue::NamedKey(named_key)) => {
                    let key = named_key.get_key().map_err(TrackingCopyError::CLValue)?;
                    let name = named_key.get_name().map_err(TrackingCopyError::CLValue)?;
                    named_keys.insert(name, key);
                }
                Some(other) => {
                    return Err(TrackingCopyError::TypeMismatch(
                        StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
                    ))
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

        Ok(named_keys)
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

    fn get_system_entity_registry(&mut self) -> Result<SystemEntityRegistry, Self::Error> {
        match self.get(&Key::SystemEntityRegistry)? {
            Some(StoredValue::CLValue(registry)) => {
                let registry: SystemEntityRegistry =
                    CLValue::into_t(registry).map_err(Self::Error::from)?;
                Ok(registry)
            }
            Some(other) => Err(TrackingCopyError::TypeMismatch(
                StoredValueTypeMismatch::new("CLValue".to_string(), other.type_name()),
            )),
            None => Err(TrackingCopyError::KeyNotFound(Key::SystemEntityRegistry)),
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
