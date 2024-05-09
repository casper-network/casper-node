use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    convert::TryInto,
};
use tracing::{error, warn};

use crate::{
    data_access_layer::balance::{
        AvailableBalanceChecker, BalanceHolds, BalanceHoldsWithProof, ProcessingHoldBalanceHandling,
    },
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{TrackingCopy, TrackingCopyError},
    KeyPrefix,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::NamedKeys,
    bytesrepr::ToBytes,
    global_state::TrieMerkleProof,
    system::{
        mint::{
            BalanceHoldAddr, BalanceHoldAddrTag, MINT_GAS_HOLD_HANDLING_KEY,
            MINT_GAS_HOLD_INTERVAL_KEY,
        },
        MINT,
    },
    BlockGlobalAddr, BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash, CLValue, ChecksumRegistry,
    EntityAddr, EntryPointAddr, EntryPointValue, EntryPoints, HoldBalanceHandling, HoldsEpoch, Key,
    KeyTag, Motes, Package, PackageHash, StoredValue, StoredValueTypeMismatch,
    SystemEntityRegistry, URef, URefAddr, U512,
};

/// Higher-level operations on the state via a `TrackingCopy`.
pub trait TrackingCopyExt<R> {
    /// The type for the returned errors.
    type Error;

    /// Reads the entity key for a given account hash.
    fn read_account_key(&mut self, account_hash: AccountHash) -> Result<Key, Self::Error>;

    /// Returns block time associated with checked out root hash.
    fn get_block_time(&self) -> Result<Option<BlockTime>, Self::Error>;

    /// Returns balance hold configuration settings for imputed kind of balance hold.
    fn get_balance_hold_config(
        &self,
        hold_kind: BalanceHoldAddrTag,
    ) -> Result<Option<(BlockTime, HoldBalanceHandling, u64)>, Self::Error>;

    /// Gets the purse balance key for a given purse.
    fn get_purse_balance_key(&self, purse_key: Key) -> Result<Key, Self::Error>;

    /// Gets the balance hold keys for the imputed purse (if any).
    fn get_balance_hold_addresses(
        &self,
        purse_addr: URefAddr,
    ) -> Result<Vec<BalanceHoldAddr>, Self::Error>;

    /// Returns total balance.
    fn get_total_balance(&self, key: Key) -> Result<Motes, Self::Error>;

    /// Returns the available balance, considering any holds from holds_epoch to now.
    fn get_available_balance(&mut self, balance_key: Key) -> Result<Motes, Self::Error>;

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
        filter: Vec<(BalanceHoldAddrTag, HoldsEpoch)>,
    ) -> Result<(), Self::Error>;

    /// Gets the balance holds for a given balance, without Merkle proofs.
    fn get_balance_holds(
        &mut self,
        purse_addr: URefAddr,
    ) -> Result<BTreeMap<BlockTime, BalanceHolds>, Self::Error>;

    /// Gets the balance holds for a given balance, with Merkle proofs.
    fn get_balance_holds_with_proof(
        &self,
        purse_addr: URefAddr,
    ) -> Result<BTreeMap<BlockTime, BalanceHoldsWithProof>, Self::Error>;

    /// Returns the collection of named keys for a given AddressableEntity.
    fn get_named_keys(&self, entity_addr: EntityAddr) -> Result<NamedKeys, Self::Error>;

    /// Returns the collection of entry points for a given AddresableEntity.
    fn get_v1_entry_points(&mut self, entity_addr: EntityAddr) -> Result<EntryPoints, Self::Error>;

    /// Gets a package by hash.
    fn get_package(&mut self, package_hash: PackageHash) -> Result<Package, Self::Error>;

    /// Gets the system entity registry.
    fn get_system_entity_registry(&self) -> Result<SystemEntityRegistry, Self::Error>;

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

    fn get_block_time(&self) -> Result<Option<BlockTime>, Self::Error> {
        match self.read(&Key::BlockGlobal(BlockGlobalAddr::BlockTime))? {
            None => Ok(None),
            Some(StoredValue::CLValue(cl_value)) => {
                let block_time = cl_value.into_t().map_err(Self::Error::CLValue)?;
                Ok(Some(BlockTime::new(block_time)))
            }
            Some(unexpected) => {
                warn!(?unexpected, "block time stored as unexpected value type");
                Err(Self::Error::UnexpectedStoredValueVariant)
            }
        }
    }

    fn get_balance_hold_config(
        &self,
        hold_kind: BalanceHoldAddrTag,
    ) -> Result<Option<(BlockTime, HoldBalanceHandling, u64)>, Self::Error> {
        let block_time = match self.get_block_time()? {
            None => return Ok(None),
            Some(block_time) => block_time,
        };
        let (handling_key, interval_key) = match hold_kind {
            BalanceHoldAddrTag::Processing => {
                return Ok(Some((block_time, HoldBalanceHandling::Accrued, 0)));
            }
            BalanceHoldAddrTag::Gas => (MINT_GAS_HOLD_HANDLING_KEY, MINT_GAS_HOLD_INTERVAL_KEY),
        };

        let system_contract_registry = self.get_system_entity_registry()?;

        let entity_hash = system_contract_registry.get(MINT).ok_or_else(|| {
            error!("Missing system mint contract hash");
            TrackingCopyError::MissingSystemContractHash(MINT.to_string())
        })?;

        let named_keys = self.get_named_keys(EntityAddr::System(entity_hash.value()))?;

        // get the handling
        let handling = {
            let named_key =
                named_keys
                    .get(handling_key)
                    .ok_or(TrackingCopyError::NamedKeyNotFound(
                        handling_key.to_string(),
                    ))?;
            let _uref = named_key
                .as_uref()
                .ok_or(TrackingCopyError::UnexpectedKeyVariant(*named_key))?;

            match self.read(&named_key.normalize()) {
                Ok(Some(StoredValue::CLValue(cl_value))) => {
                    let handling_tag = cl_value.into_t().map_err(TrackingCopyError::CLValue)?;
                    HoldBalanceHandling::from_tag(handling_tag).map_err(|_| {
                        TrackingCopyError::ValueNotFound(
                            "No hold balance handling variant matches stored tag".to_string(),
                        )
                    })?
                }
                Ok(Some(unexpected)) => {
                    warn!(
                        ?unexpected,
                        "hold balance handling unexpected stored value variant"
                    );
                    return Err(TrackingCopyError::UnexpectedStoredValueVariant);
                }
                Ok(None) => {
                    error!("hold balance handling missing from gs");
                    return Err(TrackingCopyError::ValueNotFound(handling_key.to_string()));
                }
                Err(gse) => {
                    error!(?gse, "hold balance handling read error");
                    return Err(TrackingCopyError::Storage(gse));
                }
            }
        };

        // get the interval.
        let interval = {
            let named_key =
                named_keys
                    .get(interval_key)
                    .ok_or(TrackingCopyError::NamedKeyNotFound(
                        interval_key.to_string(),
                    ))?;
            let _uref = named_key
                .as_uref()
                .ok_or(TrackingCopyError::UnexpectedKeyVariant(*named_key))?;

            match self.read(&named_key.normalize()) {
                Ok(Some(StoredValue::CLValue(cl_value))) => {
                    cl_value.into_t().map_err(TrackingCopyError::CLValue)?
                }
                Ok(Some(unexpected)) => {
                    warn!(
                        ?unexpected,
                        "hold balance interval unexpected stored value variant"
                    );
                    return Err(TrackingCopyError::UnexpectedStoredValueVariant);
                }
                Ok(None) => {
                    error!("hold balance interval missing from gs");
                    return Err(TrackingCopyError::ValueNotFound(handling_key.to_string()));
                }
                Err(gse) => return Err(TrackingCopyError::Storage(gse)),
            }
        };

        Ok(Some((block_time, handling, interval)))
    }

    fn get_purse_balance_key(&self, purse_key: Key) -> Result<Key, Self::Error> {
        let balance_key: URef = purse_key
            .into_uref()
            .ok_or(TrackingCopyError::UnexpectedKeyVariant(purse_key))?;
        Ok(Key::Balance(balance_key.addr()))
    }

    fn get_balance_hold_addresses(
        &self,
        purse_addr: URefAddr,
    ) -> Result<Vec<BalanceHoldAddr>, Self::Error> {
        let tagged_keys = {
            let mut ret: Vec<BalanceHoldAddr> = vec![];
            let gas_prefix = KeyPrefix::GasBalanceHoldsByPurse(purse_addr).to_bytes()?;
            for key in self.keys_with_prefix(&gas_prefix)? {
                let addr = key
                    .as_balance_hold()
                    .ok_or(Self::Error::UnexpectedKeyVariant(key))?;
                ret.push(*addr);
            }
            let processing_prefix =
                KeyPrefix::ProcessingBalanceHoldsByPurse(purse_addr).to_bytes()?;
            for key in self.keys_with_prefix(&processing_prefix)? {
                let addr = key
                    .as_balance_hold()
                    .ok_or(Self::Error::UnexpectedKeyVariant(key))?;
                ret.push(*addr);
            }
            ret
        };
        Ok(tagged_keys)
    }

    fn get_total_balance(&self, key: Key) -> Result<Motes, Self::Error> {
        let key = {
            if let Key::URef(uref) = key {
                Key::Balance(uref.addr())
            } else {
                key
            }
        };
        if let Key::Balance(_) = key {
            let stored_value: StoredValue = self
                .read(&key)?
                .ok_or(TrackingCopyError::KeyNotFound(key))?;
            let cl_value: CLValue = stored_value
                .try_into()
                .map_err(TrackingCopyError::TypeMismatch)?;
            let total_balance = cl_value.into_t::<U512>()?;
            Ok(Motes::new(total_balance))
        } else {
            Err(Self::Error::UnexpectedKeyVariant(key))
        }
    }

    fn get_available_balance(&mut self, key: Key) -> Result<Motes, Self::Error> {
        let purse_addr = {
            if let Key::URef(uref) = key {
                uref.addr()
            } else if let Key::Balance(uref_addr) = key {
                uref_addr
            } else {
                return Err(Self::Error::UnexpectedKeyVariant(key));
            }
        };

        let total_balance = self.get_total_balance(Key::Balance(purse_addr))?.value();
        let (block_time, handling, interval) =
            match self.get_balance_hold_config(BalanceHoldAddrTag::Gas)? {
                None => {
                    // if there is no hold config at this root hash, holds are not a thing
                    // and available balance = total balance
                    return Ok(Motes::new(total_balance));
                }
                Some((block_time, handling, interval)) => (block_time, handling, interval),
            };

        let balance_holds = self.get_balance_holds(purse_addr)?;
        let gas_handling = (handling, interval).into();
        let processing_handling = ProcessingHoldBalanceHandling::new();
        match balance_holds.available_balance(
            block_time,
            total_balance,
            gas_handling,
            processing_handling,
        ) {
            Ok(balance) => Ok(Motes::new(balance)),
            Err(balance_error) => Err(Self::Error::Balance(balance_error)),
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
        let key = {
            if let Key::URef(uref) = key {
                Key::Balance(uref.addr())
            } else {
                key
            }
        };
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
        filter: Vec<(BalanceHoldAddrTag, HoldsEpoch)>,
    ) -> Result<(), Self::Error> {
        for (tag, holds_epoch) in filter {
            let prefix = match tag {
                BalanceHoldAddrTag::Gas => KeyPrefix::GasBalanceHoldsByPurse(purse_addr),
                BalanceHoldAddrTag::Processing => {
                    KeyPrefix::ProcessingBalanceHoldsByPurse(purse_addr)
                }
            };
            let immut: &_ = self;
            let hold_keys = immut.keys_with_prefix(&prefix.to_bytes()?)?;
            for hold_key in hold_keys {
                let balance_hold_addr = hold_key
                    .as_balance_hold()
                    .ok_or(Self::Error::UnexpectedKeyVariant(hold_key))?;
                let hold_block_time = balance_hold_addr.block_time();
                if let Some(earliest_relevant_timestamp) = holds_epoch.value() {
                    if hold_block_time.value() > earliest_relevant_timestamp {
                        // skip still relevant holds
                        //  the expectation is that holds are cleared after balance checks,
                        //  and before payment settlement; if that ordering changes in the
                        //  future this strategy should be reevaluated to determine if it
                        //  remains correct.
                        continue;
                    }
                }
                // prune away holds with a timestamp newer than epoch timestamp
                //  including holds with a timestamp == epoch timestamp
                self.prune(hold_key)
            }
        }
        Ok(())
    }

    fn get_balance_holds(
        &mut self,
        purse_addr: URefAddr,
    ) -> Result<BTreeMap<BlockTime, BalanceHolds>, Self::Error> {
        // NOTE: currently there are two kinds of holds, gas and processing.
        // Processing holds only effect one block to prevent double spend and are always
        // cleared at the end of processing each transaction. Gas holds persist for some
        // interval, over many blocks and eras. Thus, using the holds_epoch for gas holds
        // during transaction execution also picks up processing holds and call sites of
        // this method currently pass the holds epoch for gas holds. This works fine for
        // now, but if one or more other kinds of holds with differing periods are added
        // in the future, this logic will need to be tweaked to take get the holds epoch
        // for each hold kind and process each kind discretely in order and collate the
        // non-expired hold total at the end.
        let mut ret: BTreeMap<BlockTime, BalanceHolds> = BTreeMap::new();
        let (block_time, interval) = match self.get_balance_hold_config(BalanceHoldAddrTag::Gas)? {
            Some((block_time, _, interval)) => (block_time.value(), interval),
            None => {
                // if there is no holds config at this root hash, there can't be any holds
                return Ok(ret);
            }
        };
        let holds_epoch = { HoldsEpoch::from_millis(block_time, interval) };
        let holds = self.get_balance_hold_addresses(purse_addr)?;
        for balance_hold_addr in holds {
            let block_time = balance_hold_addr.block_time();
            if let Some(timestamp) = holds_epoch.value() {
                if block_time.value() < timestamp {
                    // skip holds older than the interval
                    //  don't skip holds with a timestamp >= epoch timestamp
                    continue;
                }
            }
            let hold_key: Key = balance_hold_addr.into();
            let hold_amount = match self.read(&hold_key) {
                Ok(Some(StoredValue::CLValue(cl_value))) => match cl_value.into_t::<U512>() {
                    Ok(val) => val,
                    Err(cve) => return Err(Self::Error::CLValue(cve)),
                },
                Ok(Some(_)) => return Err(Self::Error::UnexpectedStoredValueVariant),
                Ok(None) => return Err(Self::Error::KeyNotFound(hold_key)),
                Err(tce) => return Err(tce),
            };
            match ret.entry(block_time) {
                Entry::Vacant(entry) => {
                    let mut inner = BTreeMap::new();
                    inner.insert(balance_hold_addr.tag(), hold_amount);
                    entry.insert(inner);
                }
                Entry::Occupied(mut occupied_entry) => {
                    let inner = occupied_entry.get_mut();
                    match inner.entry(balance_hold_addr.tag()) {
                        Entry::Vacant(entry) => {
                            entry.insert(hold_amount);
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
        Ok(ret)
    }

    fn get_balance_holds_with_proof(
        &self,
        purse_addr: URefAddr,
    ) -> Result<BTreeMap<BlockTime, BalanceHoldsWithProof>, Self::Error> {
        // NOTE: currently there are two kinds of holds, gas and processing.
        // Processing holds only effect one block to prevent double spend and are always
        // cleared at the end of processing each transaction. Gas holds persist for some
        // interval, over many blocks and eras. Thus, using the holds_epoch for gas holds
        // during transaction execution also picks up processing holds and call sites of
        // this method currently pass the holds epoch for gas holds. This works fine for
        // now, but if one or more other kinds of holds with differing periods are added
        // in the future, this logic will need to be tweaked to take get the holds epoch
        // for each hold kind and process each kind discretely in order and collate the
        // non-expired hold total at the end.
        let mut ret: BTreeMap<BlockTime, BalanceHoldsWithProof> = BTreeMap::new();
        let (block_time, interval) = match self.get_balance_hold_config(BalanceHoldAddrTag::Gas)? {
            Some((block_time, _, interval)) => (block_time.value(), interval),
            None => {
                // if there is no holds config at this root hash, there can't be any holds
                return Ok(ret);
            }
        };
        let holds_epoch = { HoldsEpoch::from_millis(block_time, interval) };
        let holds = self.get_balance_hold_addresses(purse_addr)?;
        for balance_hold_addr in holds {
            let block_time = balance_hold_addr.block_time();
            if let Some(timestamp) = holds_epoch.value() {
                if block_time.value() < timestamp {
                    // skip holds older than the interval
                    //  don't skip holds with a timestamp >= epoch timestamp
                    continue;
                }
            }
            let hold_key: Key = balance_hold_addr.into();
            let proof: TrieMerkleProof<Key, StoredValue> = self
                .read_with_proof(&hold_key.normalize())?
                .ok_or(TrackingCopyError::KeyNotFound(hold_key))?;
            let cl_value: CLValue = proof
                .value()
                .to_owned()
                .try_into()
                .map_err(TrackingCopyError::TypeMismatch)?;
            let hold_amount = cl_value.into_t()?;
            match ret.entry(block_time) {
                Entry::Vacant(entry) => {
                    let mut inner = BTreeMap::new();
                    inner.insert(balance_hold_addr.tag(), (hold_amount, proof));
                    entry.insert(inner);
                }
                Entry::Occupied(mut occupied_entry) => {
                    let inner = occupied_entry.get_mut();
                    match inner.entry(balance_hold_addr.tag()) {
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
        Ok(ret)
    }

    fn get_named_keys(&self, entity_addr: EntityAddr) -> Result<NamedKeys, Self::Error> {
        let prefix = KeyPrefix::NamedKeysByEntity(entity_addr)
            .to_bytes()
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
        let entry_points_prefix = KeyPrefix::EntryPointsV1ByEntity(entity_addr)
            .to_bytes()
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

    fn get_system_entity_registry(&self) -> Result<SystemEntityRegistry, Self::Error> {
        match self.read(&Key::SystemEntityRegistry)? {
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
}
