use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    convert::TryInto,
};

use crate::{
    data_access_layer::balance::BalanceHoldsWithProof,
    global_state::{error::Error as GlobalStateError, state::StateReader},
};
use casper_types::{
    account::AccountHash, addressable_entity::NamedKeys, global_state::TrieMerkleProof,
    system::mint::BalanceHoldAddrTag, BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash, CLValue,
    ChecksumRegistry, EntityAddr, EntryPointAddr, EntryPointValue, EntryPoints, HoldsEpoch, Key,
    KeyTag, Motes, Package, PackageHash, StoredValue, StoredValueTypeMismatch,
    SystemEntityRegistry, URef, URefAddr, U512,
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

    /// Returns the available balance, considering any holds from holds_epoch to now.
    /// If holds_epoch is none, available balance == total balance.
    fn get_available_balance(
        &self,
        balance_key: Key,
        holds_epoch: HoldsEpoch,
    ) -> Result<Motes, Self::Error>;

    /// Gets the purse balance key for a given purse and provides a Merkle proof.
    fn get_purse_balance_key_with_proof(
        &self,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Gets the balance at a given balance key and provides a Merkle proof.
    fn get_total_balance_with_proof(
        &self,
        balance_key: Key,
    ) -> Result<(U512, TrieMerkleProof<Key, StoredValue>), Self::Error>;

    /// Clear expired balance holds.
    fn clear_expired_balance_holds(
        &mut self,
        purse_addr: URefAddr,
        tag: BalanceHoldAddrTag,
        holds_epoch: HoldsEpoch,
    ) -> Result<(), Self::Error>;

    /// Gets the balance holds for a given balance, with Merkle proofs.
    fn get_balance_holds_with_proof(
        &self,
        purse_addr: URefAddr,
        holds_epoch: HoldsEpoch,
    ) -> Result<BTreeMap<BlockTime, BalanceHoldsWithProof>, Self::Error>;

    /// Returns the collection of named keys for a given AddressableEntity.
    fn get_named_keys(&mut self, entity_addr: EntityAddr) -> Result<NamedKeys, Self::Error>;

    /// Returns the collection of entry points for a given AddresableEntity.
    fn get_v1_entry_points(&mut self, entity_addr: EntityAddr) -> Result<EntryPoints, Self::Error>;

    /// Gets a package by hash.
    fn get_package(&mut self, package_hash: PackageHash) -> Result<Package, Self::Error>;

    /// Gets the system entity registry.
    fn get_system_entity_registry(&mut self) -> Result<SystemEntityRegistry, Self::Error>;

    /// Gets the system checksum registry.
    fn get_checksum_registry(&mut self) -> Result<Option<ChecksumRegistry>, Self::Error>;

    /// Gets byte code by hash.
    fn get_byte_code(&mut self, byte_code_hash: ByteCodeHash) -> Result<ByteCode, Self::Error>;
}

impl<R> TrackingCopyExt<R> for TrackingCopy<R>
    where
        R: StateReader<Key, StoredValue, Error=GlobalStateError>,
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
            .ok_or(TrackingCopyError::UnexpectedKeyVariant(purse_key))?;
        Ok(Key::Balance(balance_key.addr()))
    }

    fn get_available_balance(
        &self,
        key: Key,
        holds_epoch: HoldsEpoch,
    ) -> Result<Motes, Self::Error> {
        let key = {
            if let Key::URef(uref) = key {
                Key::Balance(uref.addr())
            } else {
                key
            }
        };

        if let Key::Balance(purse_addr) = key {
            let stored_value: StoredValue = self
                .read(&key)?
                .ok_or(TrackingCopyError::KeyNotFound(key))?;
            let cl_value: CLValue = stored_value
                .try_into()
                .map_err(TrackingCopyError::TypeMismatch)?;
            let total_balance = cl_value.into_t::<U512>()?;
            match holds_epoch.value() {
                None => Ok(Motes::new(total_balance)),
                Some(epoch) => {
                    let mut total_holds = U512::zero();
                    let tag = BalanceHoldAddrTag::Gas;
                    let prefix = tag.purse_prefix_by_tag(purse_addr)?;
                    let gas_hold_keys = self.keys_with_prefix(&prefix)?;
                    for gas_hold_key in gas_hold_keys {
                        if let Some(balance_hold_addr) = gas_hold_key.as_balance_hold() {
                            let block_time = balance_hold_addr.block_time();
                            if block_time.value() < epoch {
                                // ignore holds from prior to imputed epoch
                                continue;
                            }
                            let stored_value: StoredValue = self
                                .read(&gas_hold_key)?
                                .ok_or(TrackingCopyError::KeyNotFound(key))?;
                            let cl_value: CLValue = stored_value
                                .try_into()
                                .map_err(TrackingCopyError::TypeMismatch)?;
                            let hold_amount = cl_value.into_t()?;
                            total_holds =
                                total_holds.checked_add(hold_amount).unwrap_or(U512::zero());
                        }
                    }
                    let available = total_balance
                        .checked_sub(total_holds)
                        .unwrap_or(U512::zero());
                    Ok(Motes::new(available))
                }
            }
        } else {
            Err(Self::Error::UnexpectedKeyVariant(key))
        }
    }

    fn get_purse_balance_key_with_proof(
        &self,
        purse_key: Key,
    ) -> Result<(Key, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        let balance_key: Key = purse_key
            .uref_to_hash()
            .ok_or(TrackingCopyError::UnexpectedKeyVariant(purse_key))?;
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

    fn get_total_balance_with_proof(
        &self,
        key: Key,
    ) -> Result<(U512, TrieMerkleProof<Key, StoredValue>), Self::Error> {
        if let Key::Balance(_) = key {
            let proof: TrieMerkleProof<Key, StoredValue> = self
                .read_with_proof(&key.normalize())?
                .ok_or(TrackingCopyError::KeyNotFound(key))?;
            let cl_value: CLValue = proof
                .value()
                .to_owned()
                .try_into()
                .map_err(TrackingCopyError::TypeMismatch)?;
            let balance = cl_value.into_t()?;
            Ok((balance, proof))
        } else {
            Err(Self::Error::UnexpectedKeyVariant(key))
        }
    }

    fn clear_expired_balance_holds(
        &mut self,
        purse_addr: URefAddr,
        tag: BalanceHoldAddrTag,
        holds_epoch: HoldsEpoch,
    ) -> Result<(), Self::Error> {
        let prefix = tag.purse_prefix_by_tag(purse_addr)?;
        let immut: &_ = self;
        let gas_hold_keys = immut.keys_with_prefix(&prefix)?;
        for gas_hold_key in gas_hold_keys {
            if let Some(balance_hold_addr) = gas_hold_key.as_balance_hold() {
                let block_time = balance_hold_addr.block_time();
                if let Some(timestamp) = holds_epoch.value() {
                    if block_time.value() >= timestamp {
                        // skip still current holds
                        continue;
                    }
                }
                // prune outdated holds
                self.prune(gas_hold_key)
            }
        }
        Ok(())
    }

    fn get_balance_holds_with_proof(
        &self,
        purse_addr: URefAddr,
        holds_epoch: HoldsEpoch,
    ) -> Result<BTreeMap<BlockTime, BalanceHoldsWithProof>, Self::Error> {
        let mut ret: BTreeMap<BlockTime, BalanceHoldsWithProof> = BTreeMap::new();
        let tag = BalanceHoldAddrTag::Gas;
        let prefix = tag.purse_prefix_by_tag(purse_addr)?;
        let gas_hold_keys = self.keys_with_prefix(&prefix)?;
        // if more hold kinds are added, chain them here and loop once.
        for gas_hold_key in gas_hold_keys {
            if let Some(balance_hold_addr) = gas_hold_key.as_balance_hold() {
                let block_time = balance_hold_addr.block_time();
                if let Some(timestamp) = holds_epoch.value() {
                    if block_time.value() < timestamp {
                        // ignore holds older than the interval
                        continue;
                    }
                }
                let proof: TrieMerkleProof<Key, StoredValue> = self
                    .read_with_proof(&gas_hold_key.normalize())?
                    .ok_or(TrackingCopyError::KeyNotFound(gas_hold_key))?;
                let cl_value: CLValue = proof
                    .value()
                    .to_owned()
                    .try_into()
                    .map_err(TrackingCopyError::TypeMismatch)?;
                let hold_amount = cl_value.into_t()?;
                match ret.entry(block_time) {
                    Entry::Vacant(entry) => {
                        let mut inner = BTreeMap::new();
                        inner.insert(tag, (hold_amount, proof));
                        entry.insert(inner);
                    }
                    Entry::Occupied(mut occupied_entry) => {
                        let inner = occupied_entry.get_mut();
                        match inner.entry(tag) {
                            Entry::Vacant(entry) => {
                                entry.insert((hold_amount, proof));
                            }
                            Entry::Occupied(_) => {
                                unreachable!(
                                    "there should be only one entry per (block_time, hold kind)"
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(ret)
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

        Ok(named_keys)
    }

    fn get_v1_entry_points(&mut self, entity_addr: EntityAddr) -> Result<EntryPoints, Self::Error> {
        let entry_points_prefix = entity_addr
            .entry_points_v1_prefix()
            .map_err(Self::Error::BytesRepr)?;

        let mut ret: BTreeSet<Key> = BTreeSet::new();
        let keys = self.reader.keys_with_prefix(&entry_points_prefix)?;
        let pruned = &self.cache.prunes_cached;
        // don't include keys marked for pruning
        for key in keys {
            if pruned.contains(&key) {
                continue;
            }
            ret.insert(key);
        }

        let cache = self.cache.get_key_tag_muts_cached(&KeyTag::EntryPoint);

        // there may be newly inserted keys which have not been committed yet
        if let Some(keys) = cache {
            for key in keys {
                if ret.contains(&key) {
                    continue;
                }
                if let Key::EntryPoint(entry_point_addr) = key {
                    match entry_point_addr {
                        EntryPointAddr::VmCasperV1 { .. } => {
                            ret.insert(key);
                        }
                        EntryPointAddr::VmCasperV2 { .. } => continue,
                    }
                }
            }
        };

        let mut entry_points_v1 = EntryPoints::new();

        for entry_point_key in ret.iter() {
            match self.read(entry_point_key)? {
                Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point))) => {
                    entry_points_v1.add_entry_point(entry_point)
                }
                Some(other) => {
                    return Err(TrackingCopyError::TypeMismatch(
                        StoredValueTypeMismatch::new(
                            "EntryPointsV1".to_string(),
                            other.type_name(),
                        ),
                    ));
                }
                None => match self.cache.reads_cached.get(entry_point_key) {
                    Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point))) => {
                        entry_points_v1.add_entry_point(entry_point.to_owned())
                    }
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

        Ok(entry_points_v1)
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
