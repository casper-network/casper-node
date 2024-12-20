//! The context of execution of WASM code.

#[cfg(test)]
mod tests;

use std::{
    cell::RefCell,
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
    fmt::Debug,
    rc::Rc,
};

use tracing::error;

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::{
        AddResult, TrackingCopy, TrackingCopyCache, TrackingCopyEntityExt, TrackingCopyError,
        TrackingCopyExt,
    },
    AddressGenerator,
};

use casper_types::{
    account::{
        Account, AccountHash, AddKeyFailure, RemoveKeyFailure, SetThresholdFailure,
        UpdateKeyFailure,
    },
    addressable_entity::{
        ActionType, EntityKindTag, MessageTopicError, MessageTopics, NamedKeyAddr, NamedKeyValue,
        Weight,
    },
    bytesrepr::ToBytes,
    contract_messages::{Message, MessageAddr, MessageTopicSummary, Messages, TopicNameHash},
    contracts::{ContractHash, ContractPackage, ContractPackageHash, NamedKeys},
    execution::Effects,
    handle_stored_dictionary_value,
    system::auction::EraInfo,
    AccessRights, AddressableEntity, AddressableEntityHash, BlockTime, CLType, CLValue,
    CLValueDictionary, ContextAccessRights, Contract, EntityAddr, EntryPointAddr, EntryPointType,
    EntryPointValue, EntryPoints, Gas, GrantedAccess, HashAddr, Key, KeyTag, Motes, Package,
    PackageHash, Phase, ProtocolVersion, RuntimeArgs, RuntimeFootprint, StoredValue,
    StoredValueTypeMismatch, SystemHashRegistry, TransactionHash, Transfer, URef, URefAddr,
    DICTIONARY_ITEM_KEY_MAX_LENGTH, KEY_HASH_LENGTH, U512,
};

use crate::{
    engine_state::{BlockInfo, EngineConfig},
    execution::ExecError,
};

/// Number of bytes returned from the `random_bytes` function.
pub const RANDOM_BYTES_COUNT: usize = 32;

/// Whether the execution is permitted to call FFI `casper_add_contract_version()` or not.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AllowInstallUpgrade {
    /// Allowed.
    Allowed,
    /// Forbidden.
    Forbidden,
}

/// Holds information specific to the deployed contract.
pub struct RuntimeContext<'a, R> {
    tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    // Enables look up of specific uref based on human-readable name
    named_keys: &'a mut NamedKeys,
    // Used to check uref is known before use (prevents forging urefs)
    access_rights: ContextAccessRights,
    args: RuntimeArgs,
    authorization_keys: BTreeSet<AccountHash>,
    block_info: BlockInfo,
    transaction_hash: TransactionHash,
    gas_limit: Gas,
    gas_counter: Gas,
    address_generator: Rc<RefCell<AddressGenerator>>,
    protocol_version: ProtocolVersion,
    phase: Phase,
    engine_config: EngineConfig,
    entry_point_type: EntryPointType,
    transfers: Vec<Transfer>,
    remaining_spending_limit: U512,

    // Original account/contract for read only tasks taken before execution
    runtime_footprint: Rc<RefCell<RuntimeFootprint>>,
    // Key pointing to the account / contract / entity context this instance is tied to
    context_key: Key,
    account_hash: AccountHash,
    emit_message_cost: U512,
    allow_install_upgrade: AllowInstallUpgrade,
    payment_purse: Option<URef>,
}

impl<'a, R> RuntimeContext<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    /// Creates new runtime context where we don't already have one.
    ///
    /// Where we already have a runtime context, consider using `new_from_self()`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        named_keys: &'a mut NamedKeys,
        runtime_footprint: Rc<RefCell<RuntimeFootprint>>,
        context_key: Key,
        authorization_keys: BTreeSet<AccountHash>,
        access_rights: ContextAccessRights,
        account_hash: AccountHash,
        address_generator: Rc<RefCell<AddressGenerator>>,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        engine_config: EngineConfig,
        block_info: BlockInfo,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        phase: Phase,
        args: RuntimeArgs,
        gas_limit: Gas,
        gas_counter: Gas,
        transfers: Vec<Transfer>,
        remaining_spending_limit: U512,
        entry_point_type: EntryPointType,
        allow_install_upgrade: AllowInstallUpgrade,
    ) -> Self {
        let emit_message_cost = (*engine_config.wasm_config().v1())
            .take_host_function_costs()
            .emit_message
            .cost()
            .into();
        RuntimeContext {
            tracking_copy,
            entry_point_type,
            named_keys,
            access_rights,
            args,
            runtime_footprint,
            context_key,
            authorization_keys,
            account_hash,
            block_info,
            transaction_hash,
            gas_limit,
            gas_counter,
            address_generator,
            protocol_version,
            phase,
            engine_config,
            transfers,
            remaining_spending_limit,
            emit_message_cost,
            allow_install_upgrade,
            payment_purse: None,
        }
    }

    /// Creates new runtime context cloning values from self.
    #[allow(clippy::too_many_arguments)]
    pub fn new_from_self(
        &self,
        context_key: Key,
        entry_point_type: EntryPointType,
        named_keys: &'a mut NamedKeys,
        access_rights: ContextAccessRights,
        runtime_args: RuntimeArgs,
    ) -> Self {
        let runtime_footprint = self.runtime_footprint.clone();
        let authorization_keys = self.authorization_keys.clone();
        let account_hash = self.account_hash;

        let address_generator = self.address_generator.clone();
        let tracking_copy = self.state();
        let engine_config = self.engine_config.clone();

        let block_info = self.block_info;
        let protocol_version = self.protocol_version;
        let transaction_hash = self.transaction_hash;
        let phase = self.phase;

        let gas_limit = self.gas_limit;
        let gas_counter = self.gas_counter;
        let remaining_spending_limit = self.remaining_spending_limit();

        let transfers = self.transfers.clone();
        let payment_purse = self.payment_purse;

        RuntimeContext {
            tracking_copy,
            entry_point_type,
            named_keys,
            access_rights,
            args: runtime_args,
            runtime_footprint,
            context_key,
            authorization_keys,
            account_hash,
            block_info,
            transaction_hash,
            gas_limit,
            gas_counter,
            address_generator,
            protocol_version,
            phase,
            engine_config,
            transfers,
            remaining_spending_limit,
            emit_message_cost: self.emit_message_cost,
            allow_install_upgrade: self.allow_install_upgrade,
            payment_purse,
        }
    }

    /// Returns all authorization keys for this deploy.
    pub fn authorization_keys(&self) -> &BTreeSet<AccountHash> {
        &self.authorization_keys
    }

    /// Returns a named key by a name if it exists.
    pub fn named_keys_get(&self, name: &str) -> Option<&Key> {
        self.named_keys.get(name)
    }

    /// Returns named keys.
    pub fn named_keys(&self) -> &NamedKeys {
        self.named_keys
    }

    /// Returns a mutable reference to named keys.
    pub fn named_keys_mut(&mut self) -> &mut NamedKeys {
        self.named_keys
    }

    /// Checks if named keys contains a key referenced by name.
    pub fn named_keys_contains_key(&self, name: &str) -> bool {
        self.named_keys.contains(name)
    }

    /// Returns the payment purse, if set.
    pub fn maybe_payment_purse(&self) -> Option<URef> {
        self.payment_purse
    }

    /// Sets the payment purse to the imputed uref.
    pub fn set_payment_purse(&mut self, uref: URef) {
        self.payment_purse = Some(uref);
    }

    /// Returns an instance of the engine config.
    pub fn engine_config(&self) -> &EngineConfig {
        &self.engine_config
    }

    /// Helper function to avoid duplication in `remove_uref`.
    fn remove_key_from_contract(
        &mut self,
        key: Key,
        mut contract: Contract,
        name: &str,
    ) -> Result<(), ExecError> {
        if contract.remove_named_key(name).is_none() {
            return Ok(());
        }
        self.metered_write_gs_unsafe(key, contract)?;
        Ok(())
    }

    /// Helper function to avoid duplication in `remove_uref`.
    fn remove_key_from_entity(&mut self, name: &str) -> Result<(), ExecError> {
        let key = self.context_key;
        match key {
            Key::AddressableEntity(entity_addr) => {
                let named_key =
                    NamedKeyAddr::new_from_string(entity_addr, name.to_string())?.into();
                if let Some(StoredValue::NamedKey(_)) = self.read_gs(&named_key)? {
                    self.prune_gs_unsafe(named_key);
                }
            }
            account_hash @ Key::Account(_) => {
                let account: Account = {
                    let mut account: Account = self.read_gs_typed(&account_hash)?;
                    account.named_keys_mut().remove(name);
                    account
                };
                self.named_keys.remove(name);
                let account_value = self.account_to_validated_value(account)?;
                self.metered_write_gs_unsafe(account_hash, account_value)?;
            }
            contract_uref @ Key::URef(_) => {
                let contract: Contract = {
                    let value: StoredValue = self
                        .tracking_copy
                        .borrow_mut()
                        .read(&contract_uref)?
                        .ok_or(ExecError::KeyNotFound(contract_uref))?;

                    value.try_into().map_err(ExecError::TypeMismatch)?
                };

                self.named_keys.remove(name);
                self.remove_key_from_contract(contract_uref, contract, name)?
            }
            contract_hash @ Key::Hash(_) => {
                let contract: Contract = self.read_gs_typed(&contract_hash)?;
                self.named_keys.remove(name);
                self.remove_key_from_contract(contract_hash, contract, name)?
            }
            _ => return Err(ExecError::UnexpectedKeyVariant(key)),
        }
        Ok(())
    }

    /// Remove Key from the `named_keys` map of the current context.
    /// It removes both from the ephemeral map (RuntimeContext::named_keys) but
    /// also the to-be-persisted map (in the TrackingCopy/GlobalState).
    pub fn remove_key(&mut self, name: &str) -> Result<(), ExecError> {
        self.named_keys.remove(name);
        self.remove_key_from_entity(name)
    }

    /// Returns block info.
    pub fn get_block_info(&self) -> BlockInfo {
        self.block_info
    }

    /// Returns the transaction hash.
    pub fn get_transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    /// Extends access rights with a new map.
    pub fn access_rights_extend(&mut self, urefs: &[URef]) {
        self.access_rights.extend(urefs);
    }

    /// Returns a mapping of access rights for each [`URef`]s address.
    pub fn access_rights(&self) -> &ContextAccessRights {
        &self.access_rights
    }

    /// Returns footprint of the caller.
    pub fn runtime_footprint(&self) -> Rc<RefCell<RuntimeFootprint>> {
        Rc::clone(&self.runtime_footprint)
    }

    /// Returns arguments.
    pub fn args(&self) -> &RuntimeArgs {
        &self.args
    }

    pub(crate) fn set_args(&mut self, args: RuntimeArgs) {
        self.args = args
    }

    /// Returns new shared instance of an address generator.
    pub fn address_generator(&self) -> Rc<RefCell<AddressGenerator>> {
        Rc::clone(&self.address_generator)
    }

    /// Returns new shared instance of a tracking copy.
    pub(super) fn state(&self) -> Rc<RefCell<TrackingCopy<R>>> {
        Rc::clone(&self.tracking_copy)
    }

    /// Returns the gas limit.
    pub fn gas_limit(&self) -> Gas {
        self.gas_limit
    }

    /// Returns the current gas counter.
    pub fn gas_counter(&self) -> Gas {
        self.gas_counter
    }

    /// Sets the gas counter to a new value.
    pub fn set_gas_counter(&mut self, new_gas_counter: Gas) {
        self.gas_counter = new_gas_counter;
    }

    /// Returns the context key for this instance.
    pub fn get_context_key(&self) -> Key {
        self.context_key
    }

    /// Returns the initiator of the call chain.
    pub fn get_initiator(&self) -> AccountHash {
        self.account_hash
    }

    /// Returns the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// Returns the current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Returns `true` if the execution is permitted to call `casper_add_contract_version()`.
    pub fn install_upgrade_allowed(&self) -> bool {
        self.allow_install_upgrade == AllowInstallUpgrade::Allowed
    }

    /// Generates new deterministic hash for uses as an address.
    pub fn new_hash_address(&mut self) -> Result<[u8; KEY_HASH_LENGTH], ExecError> {
        Ok(self.address_generator.borrow_mut().new_hash_address())
    }

    /// Returns 32 pseudo random bytes.
    pub fn random_bytes(&mut self) -> Result<[u8; RANDOM_BYTES_COUNT], ExecError> {
        Ok(self.address_generator.borrow_mut().create_address())
    }

    /// Creates new [`URef`] instance.
    pub fn new_uref(&mut self, value: StoredValue) -> Result<URef, ExecError> {
        let uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.insert_uref(uref);
        self.metered_write_gs(Key::URef(uref), value)?;
        Ok(uref)
    }

    /// Creates a new URef where the value it stores is CLType::Unit.
    pub(crate) fn new_unit_uref(&mut self) -> Result<URef, ExecError> {
        self.new_uref(StoredValue::CLValue(CLValue::unit()))
    }

    /// Puts `key` to the map of named keys of current context.
    pub fn put_key(&mut self, name: String, key: Key) -> Result<(), ExecError> {
        // No need to perform actual validation on the base key because an account or contract (i.e.
        // the element stored under `base_key`) is allowed to add new named keys to itself.
        match self.get_context_key() {
            Key::Account(_) | Key::Hash(_) => {
                let named_key_value = StoredValue::CLValue(CLValue::from_t((name.clone(), key))?);
                self.validate_value(&named_key_value)?;
                self.metered_add_gs_unsafe(self.get_context_key(), named_key_value)?;
                self.insert_named_key(name, key);
            }
            Key::AddressableEntity(entity_addr) => {
                let named_key_value =
                    StoredValue::NamedKey(NamedKeyValue::from_concrete_values(key, name.clone())?);
                self.validate_value(&named_key_value)?;
                let named_key_addr = NamedKeyAddr::new_from_string(entity_addr, name.clone())?;
                self.metered_write_gs_unsafe(Key::NamedKey(named_key_addr), named_key_value)?;
                self.insert_named_key(name, key);
            }
            _ => return Err(ExecError::InvalidContext),
        }

        Ok(())
    }

    pub(crate) fn get_message_topics(
        &mut self,
        hash_addr: HashAddr,
    ) -> Result<MessageTopics, ExecError> {
        self.tracking_copy
            .borrow_mut()
            .get_message_topics(hash_addr)
            .map_err(Into::into)
    }

    pub(crate) fn get_named_keys(&mut self, entity_key: Key) -> Result<NamedKeys, ExecError> {
        let entity_addr = if let Key::AddressableEntity(entity_addr) = entity_key {
            entity_addr
        } else {
            return Err(ExecError::UnexpectedKeyVariant(entity_key));
        };
        self.tracking_copy
            .borrow_mut()
            .get_named_keys(entity_addr)
            .map_err(Into::into)
    }

    pub(crate) fn write_entry_points(
        &mut self,
        entity_addr: EntityAddr,
        entry_points: EntryPoints,
    ) -> Result<(), ExecError> {
        if entry_points.is_empty() {
            return Ok(());
        }

        for entry_point in entry_points.take_entry_points() {
            let entry_point_addr =
                EntryPointAddr::new_v1_entry_point_addr(entity_addr, entry_point.name())?;
            let entry_point_value =
                StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point));
            self.metered_write_gs_unsafe(Key::EntryPoint(entry_point_addr), entry_point_value)?;
        }

        Ok(())
    }

    pub(crate) fn get_casper_vm_v1_entry_point(
        &mut self,
        entity_key: Key,
    ) -> Result<EntryPoints, ExecError> {
        let entity_addr = if let Key::AddressableEntity(entity_addr) = entity_key {
            entity_addr
        } else {
            return Err(ExecError::UnexpectedKeyVariant(entity_key));
        };

        self.tracking_copy
            .borrow_mut()
            .get_v1_entry_points(entity_addr)
            .map_err(Into::into)
    }

    /// Reads the total balance of a purse [`URef`].
    ///
    /// Currently address of a purse [`URef`] is also a hash in the [`Key::Hash`] space.
    pub(crate) fn total_balance(&mut self, purse_uref: &URef) -> Result<Motes, ExecError> {
        let key = Key::URef(*purse_uref);
        let total = self
            .tracking_copy
            .borrow_mut()
            .get_total_balance(key)
            .map_err(ExecError::TrackingCopy)?;
        Ok(total)
    }

    /// Reads the available balance of a purse [`URef`].
    ///
    /// Currently address of a purse [`URef`] is also a hash in the [`Key::Hash`] space.
    pub(crate) fn available_balance(&mut self, purse_uref: &URef) -> Result<Motes, ExecError> {
        let key = Key::URef(*purse_uref);
        self.tracking_copy
            .borrow_mut()
            .get_available_balance(key)
            .map_err(ExecError::TrackingCopy)
    }

    /// Read a stored value under a [`Key`].
    pub fn read_gs(&mut self, key: &Key) -> Result<Option<StoredValue>, ExecError> {
        self.validate_readable(key)?;
        self.validate_key(key)?;

        let maybe_stored_value = self.tracking_copy.borrow_mut().read(key)?;

        let stored_value = match maybe_stored_value {
            Some(stored_value) => handle_stored_dictionary_value(*key, stored_value)?,
            None => return Ok(None),
        };

        Ok(Some(stored_value))
    }

    /// Reads a value from a global state directly.
    ///
    /// # Usage
    ///
    /// DO NOT EXPOSE THIS VIA THE FFI - This function bypasses security checks and should be used
    /// with caution.
    pub fn read_gs_unsafe(&mut self, key: &Key) -> Result<Option<StoredValue>, ExecError> {
        self.tracking_copy
            .borrow_mut()
            .read(key)
            .map_err(Into::into)
    }

    /// This method is a wrapper over `read_gs` in the sense that it extracts the type held by a
    /// `StoredValue` stored in the global state in a type safe manner.
    ///
    /// This is useful if you want to get the exact type from global state.
    pub fn read_gs_typed<T>(&mut self, key: &Key) -> Result<T, ExecError>
    where
        T: TryFrom<StoredValue, Error = StoredValueTypeMismatch>,
        T::Error: Debug,
    {
        let value = match self.read_gs(key)? {
            None => return Err(ExecError::KeyNotFound(*key)),
            Some(value) => value,
        };

        value
            .try_into()
            .map_err(|error| ExecError::TrackingCopy(TrackingCopyError::TypeMismatch(error)))
    }

    /// Returns all keys based on the tag prefix.
    pub fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, ExecError> {
        self.tracking_copy
            .borrow_mut()
            .get_keys(key_tag)
            .map_err(Into::into)
    }

    /// Returns all key's that start with prefix, if any.
    pub fn get_keys_with_prefix(&mut self, prefix: &[u8]) -> Result<Vec<Key>, ExecError> {
        self.tracking_copy
            .borrow_mut()
            .reader()
            .keys_with_prefix(prefix)
            .map_err(Into::into)
    }

    /// Write an era info instance to the global state.
    pub fn write_era_info(&mut self, key: Key, value: EraInfo) {
        if let Key::EraSummary = key {
            // Writing an `EraInfo` for 100 validators will not exceed write size limit.
            self.tracking_copy
                .borrow_mut()
                .write(key, StoredValue::EraInfo(value));
        } else {
            panic!("Do not use this function for writing non-era-info keys")
        }
    }

    /// Creates validated instance of `StoredValue` from `account`.
    fn account_to_validated_value(&self, account: Account) -> Result<StoredValue, ExecError> {
        let value = StoredValue::Account(account);
        self.validate_value(&value)?;
        Ok(value)
    }

    /// Write an account to the global state.
    pub fn write_account(&mut self, key: Key, account: Account) -> Result<(), ExecError> {
        if let Key::Account(_) = key {
            self.validate_key(&key)?;
            let account_value = self.account_to_validated_value(account)?;
            self.metered_write_gs_unsafe(key, account_value)?;
            Ok(())
        } else {
            panic!("Do not use this function for writing non-account keys")
        }
    }

    /// Read an account from the global state.
    pub fn read_account(&mut self, key: &Key) -> Result<Option<StoredValue>, ExecError> {
        if let Key::Account(_) = key {
            self.validate_key(key)?;
            self.tracking_copy
                .borrow_mut()
                .read(key)
                .map_err(Into::into)
        } else {
            panic!("Do not use this function for reading from non-account keys")
        }
    }

    /// Adds a named key.
    ///
    /// If given `Key` refers to an [`URef`] then it extends the runtime context's access rights
    /// with the URef's access rights.
    fn insert_named_key(&mut self, name: String, key: Key) {
        if let Key::URef(uref) = key {
            self.insert_uref(uref);
        }
        self.named_keys.insert(name, key);
    }

    /// Adds a new [`URef`] into the context.
    ///
    /// Once an [`URef`] is inserted, it's considered a valid [`URef`] in this runtime context.
    fn insert_uref(&mut self, uref: URef) {
        self.access_rights.extend(&[uref])
    }

    /// Grants access to a [`URef`]; unless access was pre-existing.
    pub fn grant_access(&mut self, uref: URef) -> GrantedAccess {
        self.access_rights.grant_access(uref)
    }

    /// Removes an access right from the current runtime context.
    pub fn remove_access(&mut self, uref_addr: URefAddr, access_rights: AccessRights) {
        self.access_rights.remove_access(uref_addr, access_rights)
    }

    /// Returns a copy of the current effects of a tracking copy.
    pub fn effects(&self) -> Effects {
        self.tracking_copy.borrow().effects()
    }

    /// Returns a copy of the current messages of a tracking copy.
    pub fn messages(&self) -> Messages {
        self.tracking_copy.borrow().messages()
    }

    /// Returns a copy of the current named keys of a tracking copy.
    pub fn cache(&self) -> TrackingCopyCache {
        self.tracking_copy.borrow().cache()
    }

    /// Returns the cost charged for the last emitted message.
    pub fn emit_message_cost(&self) -> U512 {
        self.emit_message_cost
    }

    /// Sets the cost charged for the last emitted message.
    pub fn set_emit_message_cost(&mut self, cost: U512) {
        self.emit_message_cost = cost
    }

    /// Returns list of transfers.
    pub fn transfers(&self) -> &Vec<Transfer> {
        &self.transfers
    }

    /// Returns mutable list of transfers.
    pub fn transfers_mut(&mut self) -> &mut Vec<Transfer> {
        &mut self.transfers
    }

    fn validate_cl_value(&self, cl_value: &CLValue) -> Result<(), ExecError> {
        match cl_value.cl_type() {
            CLType::Bool
            | CLType::I32
            | CLType::I64
            | CLType::U8
            | CLType::U32
            | CLType::U64
            | CLType::U128
            | CLType::U256
            | CLType::U512
            | CLType::Unit
            | CLType::String
            | CLType::Option(_)
            | CLType::List(_)
            | CLType::ByteArray(..)
            | CLType::Result { .. }
            | CLType::Map { .. }
            | CLType::Tuple1(_)
            | CLType::Tuple3(_)
            | CLType::Any
            | CLType::PublicKey => Ok(()),
            CLType::Key => {
                let key: Key = cl_value.to_t()?;
                self.validate_key(&key)
            }
            CLType::URef => {
                let uref: URef = cl_value.to_t()?;
                self.validate_uref(&uref)
            }
            tuple @ CLType::Tuple2(_) if *tuple == casper_types::named_key_type() => {
                let (_name, key): (String, Key) = cl_value.to_t()?;
                self.validate_key(&key)
            }
            CLType::Tuple2(_) => Ok(()),
        }
    }

    /// Validates whether keys used in the `value` are not forged.
    pub(crate) fn validate_value(&self, value: &StoredValue) -> Result<(), ExecError> {
        match value {
            StoredValue::CLValue(cl_value) => self.validate_cl_value(cl_value),
            StoredValue::NamedKey(named_key_value) => {
                self.validate_cl_value(named_key_value.get_key_as_cl_value())?;
                self.validate_cl_value(named_key_value.get_name_as_cl_value())
            }
            StoredValue::Account(_)
            | StoredValue::ByteCode(_)
            | StoredValue::Contract(_)
            | StoredValue::AddressableEntity(_)
            | StoredValue::SmartContract(_)
            | StoredValue::Transfer(_)
            | StoredValue::DeployInfo(_)
            | StoredValue::EraInfo(_)
            | StoredValue::Bid(_)
            | StoredValue::BidKind(_)
            | StoredValue::Withdraw(_)
            | StoredValue::Unbonding(_)
            | StoredValue::ContractPackage(_)
            | StoredValue::ContractWasm(_)
            | StoredValue::MessageTopic(_)
            | StoredValue::Message(_)
            | StoredValue::Prepayment(_)
            | StoredValue::EntryPoint(_)
            | StoredValue::RawBytes(_) => Ok(()),
        }
    }

    pub(crate) fn context_key_to_entity_addr(&self) -> Result<EntityAddr, ExecError> {
        match self.context_key {
            Key::Account(account_hash) => Ok(EntityAddr::Account(account_hash.value())),
            Key::Hash(hash) => {
                if self.is_system_addressable_entity(&hash)? {
                    Ok(EntityAddr::System(hash))
                } else {
                    Ok(EntityAddr::SmartContract(hash))
                }
            }
            Key::AddressableEntity(addr) => Ok(addr),
            _ => Err(ExecError::UnexpectedKeyVariant(self.context_key)),
        }
    }

    /// Validates whether key is not forged (whether it can be found in the
    /// `named_keys`) and whether the version of a key that contract wants
    /// to use, has access rights that are less powerful than access rights'
    /// of the key in the `named_keys`.
    pub(crate) fn validate_key(&self, key: &Key) -> Result<(), ExecError> {
        let uref = match key {
            Key::URef(uref) => uref,
            _ => return Ok(()),
        };
        self.validate_uref(uref)
    }

    /// Validate [`URef`] access rights.
    ///
    /// Returns unit if [`URef`]s address exists in the context, and has correct access rights bit
    /// set.
    pub(crate) fn validate_uref(&self, uref: &URef) -> Result<(), ExecError> {
        if self.access_rights.has_access_rights_to_uref(uref) {
            Ok(())
        } else {
            Err(ExecError::ForgedReference(*uref))
        }
    }

    /// Validates if a [`Key`] refers to a [`URef`] and has a read bit set.
    fn validate_readable(&self, key: &Key) -> Result<(), ExecError> {
        if self.is_readable(key) {
            Ok(())
        } else {
            Err(ExecError::InvalidAccess {
                required: AccessRights::READ,
            })
        }
    }

    /// Validates if a [`Key`] refers to a [`URef`] and has a add bit set.
    fn validate_addable(&self, key: &Key) -> Result<(), ExecError> {
        if self.is_addable(key) {
            Ok(())
        } else {
            Err(ExecError::InvalidAccess {
                required: AccessRights::ADD,
            })
        }
    }

    /// Validates if a [`Key`] refers to a [`URef`] and has a write bit set.
    pub(crate) fn validate_writeable(&self, key: &Key) -> Result<(), ExecError> {
        if self.is_writeable(key) {
            Ok(())
        } else {
            Err(ExecError::InvalidAccess {
                required: AccessRights::WRITE,
            })
        }
    }

    /// Tests whether reading from the `key` is valid.
    pub fn is_readable(&self, key: &Key) -> bool {
        match self.context_key_to_entity_addr() {
            Ok(entity_addr) => key.is_readable(&entity_addr),
            Err(error) => {
                error!(?error, "entity_key is unexpected key variant");
                panic!("is_readable: entity_key is unexpected key variant");
            }
        }
    }

    /// Tests whether addition to `key` is valid.
    pub fn is_addable(&self, key: &Key) -> bool {
        match self.context_key_to_entity_addr() {
            Ok(entity_addr) => key.is_addable(&entity_addr),
            Err(error) => {
                error!(?error, "entity_key is unexpected key variant");
                panic!("is_readable: entity_key is unexpected key variant");
            }
        }
    }

    /// Tests whether writing to `key` is valid.
    pub fn is_writeable(&self, key: &Key) -> bool {
        match self.context_key_to_entity_addr() {
            Ok(entity_addr) => key.is_writeable(&entity_addr),
            Err(error) => {
                error!(?error, "entity_key is unexpected key variant");
                panic!("is_readable: entity_key is unexpected key variant");
            }
        }
    }

    /// Safely charge the specified amount of gas, up to the available gas limit.
    ///
    /// Returns [`Error::GasLimit`] if gas limit exceeded and `()` if not.
    /// Intuition about the return value sense is to answer the question 'are we
    /// allowed to continue?'
    pub(crate) fn charge_gas(&mut self, gas: Gas) -> Result<(), ExecError> {
        let prev = self.gas_counter();
        let gas_limit = self.gas_limit();
        // gas charge overflow protection
        match prev.checked_add(gas) {
            None => {
                self.set_gas_counter(gas_limit);
                Err(ExecError::GasLimit)
            }
            Some(val) if val > gas_limit => {
                self.set_gas_counter(gas_limit);
                Err(ExecError::GasLimit)
            }
            Some(val) => {
                self.set_gas_counter(val);
                Ok(())
            }
        }
    }

    /// Checks if we are calling a system addressable entity.
    pub(crate) fn is_system_addressable_entity(
        &self,
        hash_addr: &HashAddr,
    ) -> Result<bool, ExecError> {
        Ok(self.system_entity_registry()?.exists(hash_addr))
    }

    /// Charges gas for specified amount of bytes used.
    fn charge_gas_storage(&mut self, bytes_count: usize) -> Result<(), ExecError> {
        if let Some(hash_addr) = self.get_context_key().into_entity_hash_addr() {
            if self.is_system_addressable_entity(&hash_addr)? {
                // Don't charge storage used while executing a system contract.
                return Ok(());
            }
        }

        let storage_costs = self.engine_config.storage_costs();

        let gas_cost = storage_costs.calculate_gas_cost(bytes_count);

        self.charge_gas(gas_cost)
    }

    /// Charges gas for using a host system contract's entrypoint.
    pub(crate) fn charge_system_contract_call<T>(&mut self, call_cost: T) -> Result<(), ExecError>
    where
        T: Into<Gas>,
    {
        let amount: Gas = call_cost.into();
        self.charge_gas(amount)
    }

    /// Prune a key from the global state.
    ///
    /// Use with caution - there is no validation done as the key is assumed to be validated
    /// already.
    pub(crate) fn prune_gs_unsafe<K>(&mut self, key: K)
    where
        K: Into<Key>,
    {
        self.tracking_copy.borrow_mut().prune(key.into());
    }

    pub(crate) fn migrate_package(
        &mut self,
        contract_package_hash: ContractPackageHash,
        protocol_version: ProtocolVersion,
    ) -> Result<(), ExecError> {
        self.tracking_copy
            .borrow_mut()
            .migrate_package(Key::Hash(contract_package_hash.value()), protocol_version)
            .map_err(ExecError::TrackingCopy)
    }

    /// Writes data to global state with a measurement.
    ///
    /// Use with caution - there is no validation done as the key is assumed to be validated
    /// already.
    pub(crate) fn metered_write_gs_unsafe<K, V>(
        &mut self,
        key: K,
        value: V,
    ) -> Result<(), ExecError>
    where
        K: Into<Key>,
        V: Into<StoredValue>,
    {
        let stored_value = value.into();

        // Charge for amount as measured by serialized length
        let bytes_count = stored_value.serialized_length();
        self.charge_gas_storage(bytes_count)?;

        self.tracking_copy
            .borrow_mut()
            .write(key.into(), stored_value);
        Ok(())
    }

    /// Emits message and writes message summary to global state with a measurement.
    pub(crate) fn metered_emit_message(
        &mut self,
        topic_key: Key,
        block_time: BlockTime,
        block_message_count: u64,
        topic_message_count: u32,
        message: Message,
    ) -> Result<(), ExecError> {
        let topic_value = StoredValue::MessageTopic(MessageTopicSummary::new(
            topic_message_count,
            block_time,
            message.topic_name().clone(),
        ));
        let message_key = message.message_key();
        let message_value = StoredValue::Message(message.checksum().map_err(ExecError::BytesRepr)?);

        let block_message_count_value =
            StoredValue::CLValue(CLValue::from_t((block_time, block_message_count))?);

        // Charge for amount as measured by serialized length
        let bytes_count = topic_value.serialized_length()
            + message_value.serialized_length()
            + block_message_count_value.serialized_length();
        self.charge_gas_storage(bytes_count)?;

        self.tracking_copy.borrow_mut().emit_message(
            topic_key,
            topic_value,
            message_key,
            message_value,
            block_message_count_value,
            message,
        );
        Ok(())
    }

    /// Writes data to a global state and charges for bytes stored.
    ///
    /// This method performs full validation of the key to be written.
    pub(crate) fn metered_write_gs<T>(&mut self, key: Key, value: T) -> Result<(), ExecError>
    where
        T: Into<StoredValue>,
    {
        let stored_value = value.into();
        self.validate_writeable(&key)?;
        self.validate_key(&key)?;
        self.validate_value(&stored_value)?;
        self.metered_write_gs_unsafe(key, stored_value)
    }

    /// Adds data to a global state key and charges for bytes stored.
    ///
    /// This method performs full validation of the key to be written.
    pub(crate) fn metered_add_gs_unsafe(
        &mut self,
        key: Key,
        value: StoredValue,
    ) -> Result<(), ExecError> {
        let value_bytes_count = value.serialized_length();
        self.charge_gas_storage(value_bytes_count)?;

        match self.tracking_copy.borrow_mut().add(key, value) {
            Err(storage_error) => Err(storage_error.into()),
            Ok(AddResult::Success) => Ok(()),
            Ok(AddResult::KeyNotFound(key)) => Err(ExecError::KeyNotFound(key)),
            Ok(AddResult::TypeMismatch(type_mismatch)) => {
                Err(ExecError::TypeMismatch(type_mismatch))
            }
            Ok(AddResult::Serialization(error)) => Err(ExecError::BytesRepr(error)),
            Ok(AddResult::Transform(error)) => Err(ExecError::Transform(error)),
        }
    }

    /// Adds `value` to the `key`. The premise for being able to `add` value is
    /// that the type of it value can be added (is a Monoid). If the
    /// values can't be added, either because they're not a Monoid or if the
    /// value stored under `key` has different type, then `TypeMismatch`
    /// errors is returned.
    pub(crate) fn metered_add_gs<K, V>(&mut self, key: K, value: V) -> Result<(), ExecError>
    where
        K: Into<Key>,
        V: Into<StoredValue>,
    {
        let key = key.into();
        let value = value.into();
        self.validate_addable(&key)?;
        self.validate_key(&key)?;
        self.validate_value(&value)?;
        self.metered_add_gs_unsafe(key, value)
    }

    /// Adds new associated key.
    pub(crate) fn add_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), ExecError> {
        let context_key = self.context_key;
        let entity_addr = match self.context_key_to_entity_addr() {
            Ok(entity_addr) => entity_addr,
            Err(error) => return Err(error),
        };

        if EntryPointType::Caller == self.entry_point_type
            && entity_addr.tag() != EntityKindTag::Account
        {
            // Exit early with error to avoid mutations
            return Err(AddKeyFailure::PermissionDenied.into());
        }

        if self.engine_config.enable_entity {
            // Get the current entity record
            let entity = {
                let mut entity: AddressableEntity = self.read_gs_typed(&context_key)?;
                // enforce max keys limit
                if entity.associated_keys().len()
                    >= (self.engine_config.max_associated_keys() as usize)
                {
                    return Err(ExecError::AddKeyFailure(AddKeyFailure::MaxKeysLimit));
                }

                // Exit early in case of error without updating global state
                entity
                    .add_associated_key(account_hash, weight)
                    .map_err(ExecError::from)?;
                entity
            };

            self.metered_write_gs_unsafe(
                context_key,
                self.addressable_entity_to_validated_value(entity)?,
            )?;
        } else {
            // Take an account out of the global state
            let account = {
                let mut account: Account = self.read_gs_typed(&context_key)?;

                if account.associated_keys().len() as u32
                    >= (self.engine_config.max_associated_keys())
                {
                    return Err(ExecError::AddKeyFailure(AddKeyFailure::MaxKeysLimit));
                }

                // Exit early in case of error without updating global state
                let result = account.add_associated_key(
                    account_hash,
                    casper_types::account::Weight::new(weight.value()),
                );

                result.map_err(ExecError::from)?;
                account
            };

            let account_value = self.account_to_validated_value(account)?;

            self.metered_write_gs_unsafe(context_key, account_value)?;
        }

        Ok(())
    }

    /// Remove associated key.
    pub(crate) fn remove_associated_key(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<(), ExecError> {
        let context_key = self.context_key;
        let entity_addr = match self.context_key_to_entity_addr() {
            Ok(entity_addr) => entity_addr,
            Err(error) => return Err(error),
        };

        if EntryPointType::Caller == self.entry_point_type
            && entity_addr.tag() != EntityKindTag::Account
        {
            // Exit early with error to avoid mutations
            return Err(RemoveKeyFailure::PermissionDenied.into());
        }

        if !self
            .runtime_footprint()
            .borrow()
            .can_manage_keys_with(&self.authorization_keys)
        {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(RemoveKeyFailure::PermissionDenied.into());
        }

        if self.engine_config.enable_entity {
            // Get the current entity record
            let entity = {
                let mut entity: AddressableEntity = self.read_gs_typed(&context_key)?;
                // enforce max keys limit
                if entity.associated_keys().len()
                    >= (self.engine_config.max_associated_keys() as usize)
                {
                    return Err(ExecError::AddKeyFailure(AddKeyFailure::MaxKeysLimit));
                }

                // Exit early in case of error without updating global state
                entity
                    .remove_associated_key(account_hash)
                    .map_err(ExecError::from)?;
                entity
            };

            self.metered_write_gs_unsafe(
                context_key,
                self.addressable_entity_to_validated_value(entity)?,
            )?;
        } else {
            // Take an account out of the global state
            let account = {
                let mut account: Account = self.read_gs_typed(&context_key)?;

                if account.associated_keys().len()
                    >= (self.engine_config.max_associated_keys() as usize)
                {
                    return Err(ExecError::AddKeyFailure(AddKeyFailure::MaxKeysLimit));
                }

                // Exit early in case of error without updating global state
                account
                    .remove_associated_key(account_hash)
                    .map_err(ExecError::from)?;
                account
            };

            let account_value = self.account_to_validated_value(account)?;

            self.metered_write_gs_unsafe(context_key, account_value)?;
        }

        Ok(())
    }

    /// Update associated key.
    pub(crate) fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), ExecError> {
        let context_key = self.context_key;
        let entity_addr = match self.context_key_to_entity_addr() {
            Ok(entity_addr) => entity_addr,
            Err(error) => return Err(error),
        };

        if EntryPointType::Caller == self.entry_point_type
            && entity_addr.tag() != EntityKindTag::Account
        {
            // Exit early with error to avoid mutations
            return Err(UpdateKeyFailure::PermissionDenied.into());
        }

        if !self
            .runtime_footprint()
            .borrow()
            .can_manage_keys_with(&self.authorization_keys)
        {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(UpdateKeyFailure::PermissionDenied.into());
        }

        if self.engine_config.enable_entity {
            // Get the current entity record
            let entity = {
                let mut entity: AddressableEntity = self.read_gs_typed(&context_key)?;

                // Exit early in case of error without updating global state
                entity
                    .update_associated_key(account_hash, weight)
                    .map_err(ExecError::from)?;
                entity
            };

            self.metered_write_gs_unsafe(
                context_key,
                self.addressable_entity_to_validated_value(entity)?,
            )?;
        } else {
            // Take an account out of the global state
            let account = {
                let mut account: Account = self.read_gs_typed(&context_key)?;

                // Exit early in case of error without updating global state
                account
                    .update_associated_key(
                        account_hash,
                        casper_types::account::Weight::new(weight.value()),
                    )
                    .map_err(ExecError::from)?;
                account
            };

            let account_value = self.account_to_validated_value(account)?;

            self.metered_write_gs_unsafe(context_key, account_value)?;
        }

        Ok(())
    }

    pub(crate) fn is_authorized_by_admin(&self) -> bool {
        self.engine_config
            .administrative_accounts()
            .intersection(&self.authorization_keys)
            .next()
            .is_some()
    }
    /// Gets given contract package with its access_key validated against current context.
    pub(crate) fn get_validated_contract_package(
        &mut self,
        package_hash: HashAddr,
    ) -> Result<ContractPackage, ExecError> {
        let package_hash_key = Key::Hash(package_hash);
        self.validate_key(&package_hash_key)?;
        let contract_package: ContractPackage = self.read_gs_typed(&package_hash_key)?;

        if !self.is_authorized_by_admin() {
            self.validate_uref(&contract_package.access_key())?;
        }

        Ok(contract_package)
    }

    /// Set threshold of an associated key.
    pub(crate) fn set_action_threshold(
        &mut self,
        action_type: ActionType,
        threshold: Weight,
    ) -> Result<(), ExecError> {
        let context_key = self.context_key;
        let entity_addr = match self.context_key_to_entity_addr() {
            Ok(entity_addr) => entity_addr,
            Err(error) => return Err(error),
        };

        if EntryPointType::Caller == self.entry_point_type
            && entity_addr.tag() != EntityKindTag::Account
        {
            // Exit early with error to avoid mutations
            return Err(SetThresholdFailure::PermissionDeniedError.into());
        }

        if self.engine_config.enable_entity {
            // Take an addressable entity out of the global state
            let mut entity: AddressableEntity = self.read_gs_typed(&context_key)?;

            // Exit early in case of error without updating global state
            if self.is_authorized_by_admin() {
                entity.set_action_threshold_unchecked(action_type, threshold)
            } else {
                entity.set_action_threshold(action_type, threshold)
            }
            .map_err(ExecError::from)?;

            let entity_value = self.addressable_entity_to_validated_value(entity)?;

            self.metered_write_gs_unsafe(context_key, entity_value)?;
        } else {
            // Converts an account's public key into a URef
            let key = Key::Account(AccountHash::new(entity_addr.value()));

            // Take an account out of the global state
            let mut account: Account = self.read_gs_typed(&key)?;

            // Exit early in case of error without updating global state
            let action_type = match action_type {
                ActionType::Deployment => casper_types::account::ActionType::Deployment,
                ActionType::KeyManagement => casper_types::account::ActionType::KeyManagement,
                ActionType::UpgradeManagement => return Err(ExecError::InvalidContext),
            };

            let threshold = casper_types::account::Weight::new(threshold.value());

            if self.is_authorized_by_admin() {
                account.set_action_threshold_unchecked(action_type, threshold)
            } else {
                account.set_action_threshold(action_type, threshold)
            }
            .map_err(ExecError::from)?;

            let account_value = self.account_to_validated_value(account)?;

            self.metered_write_gs_unsafe(key, account_value)?;
        }

        Ok(())
    }

    fn addressable_entity_to_validated_value(
        &self,
        entity: AddressableEntity,
    ) -> Result<StoredValue, ExecError> {
        let value = StoredValue::AddressableEntity(entity);
        self.validate_value(&value)?;
        Ok(value)
    }

    pub(crate) fn runtime_footprint_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<Option<RuntimeFootprint>, ExecError> {
        if self.engine_config.enable_entity {
            match self.read_gs(&Key::Account(account_hash))? {
                Some(StoredValue::CLValue(cl_value)) => {
                    let key: Key = cl_value.into_t().map_err(ExecError::CLValue)?;
                    match self.read_gs(&key)? {
                        Some(StoredValue::AddressableEntity(addressable_entity)) => {
                            let entity_addr = EntityAddr::Account(account_hash.value());
                            let named_keys = self.get_named_keys(key)?;
                            let entry_points = self.get_casper_vm_v1_entry_point(key)?;
                            let footprint = RuntimeFootprint::new_entity_footprint(
                                entity_addr,
                                addressable_entity,
                                named_keys,
                                entry_points,
                            );
                            Ok(Some(footprint))
                        }
                        Some(_other_variant_2) => Err(ExecError::UnexpectedStoredValueVariant),
                        None => Ok(None),
                    }
                }
                Some(_other_variant_1) => Err(ExecError::UnexpectedStoredValueVariant),
                None => Ok(None),
            }
        } else {
            match self.read_gs(&Key::Account(account_hash))? {
                Some(StoredValue::Account(account)) => {
                    Ok(Some(RuntimeFootprint::new_account_footprint(account)))
                }
                Some(_other_variant_1) => Err(ExecError::UnexpectedStoredValueVariant),
                None => Ok(None),
            }
        }
    }

    /// Gets main purse id
    pub fn get_main_purse(&mut self) -> Result<URef, ExecError> {
        let main_purse = self
            .runtime_footprint()
            .borrow()
            .main_purse()
            .ok_or_else(|| ExecError::InvalidContext)?;
        Ok(main_purse)
    }

    /// Gets entry point type.
    pub fn entry_point_type(&self) -> EntryPointType {
        self.entry_point_type
    }

    /// Gets given contract package with its access_key validated against current context.
    pub(crate) fn get_validated_package(
        &mut self,
        package_hash: PackageHash,
    ) -> Result<Package, ExecError> {
        let package_hash_key = Key::from(package_hash);
        self.validate_key(&package_hash_key)?;
        let contract_package = if self.engine_config.enable_entity {
            self.read_gs_typed::<Package>(&Key::SmartContract(package_hash.value()))?
        } else {
            let cp = self.read_gs_typed::<ContractPackage>(&Key::Hash(package_hash.value()))?;
            cp.into()
        };
        Ok(contract_package)
    }

    pub(crate) fn get_package(&mut self, package_hash: HashAddr) -> Result<Package, ExecError> {
        self.tracking_copy
            .borrow_mut()
            .get_package(package_hash)
            .map_err(Into::into)
    }

    pub(crate) fn get_contract(
        &mut self,
        contract_hash: ContractHash,
    ) -> Result<Contract, ExecError> {
        self.tracking_copy
            .borrow_mut()
            .get_contract(contract_hash)
            .map_err(Into::into)
    }

    pub(crate) fn get_contract_entity(
        &mut self,
        entity_key: Key,
    ) -> Result<(AddressableEntity, bool), ExecError> {
        let entity_hash = if let Some(entity_hash) = entity_key.into_entity_hash() {
            entity_hash
        } else {
            return Err(ExecError::UnexpectedKeyVariant(entity_key));
        };

        let mut tc = self.tracking_copy.borrow_mut();

        let key = Key::contract_entity_key(entity_hash);
        match tc.read(&key)? {
            Some(StoredValue::AddressableEntity(entity)) => Ok((entity, false)),
            Some(other) => Err(ExecError::TypeMismatch(StoredValueTypeMismatch::new(
                "AddressableEntity".to_string(),
                other.type_name(),
            ))),
            None => match tc.read(&Key::Hash(entity_hash.value()))? {
                Some(StoredValue::Contract(contract)) => Ok((contract.into(), true)),
                Some(other) => Err(ExecError::TypeMismatch(StoredValueTypeMismatch::new(
                    "Contract".to_string(),
                    other.type_name(),
                ))),
                None => Err(TrackingCopyError::KeyNotFound(key).into()),
            },
        }
    }

    /// Gets a dictionary item key from a dictionary referenced by a `uref`.
    pub(crate) fn dictionary_get(
        &mut self,
        uref: URef,
        dictionary_item_key: &str,
    ) -> Result<Option<CLValue>, ExecError> {
        self.validate_readable(&uref.into())?;
        self.validate_key(&uref.into())?;
        let dictionary_item_key_bytes = dictionary_item_key.as_bytes();

        if dictionary_item_key_bytes.len() > DICTIONARY_ITEM_KEY_MAX_LENGTH {
            return Err(ExecError::DictionaryItemKeyExceedsLength);
        }

        let dictionary_key = Key::dictionary(uref, dictionary_item_key_bytes);
        self.dictionary_read(dictionary_key)
    }

    /// Gets a dictionary value from a dictionary `Key`.
    pub(crate) fn dictionary_read(
        &mut self,
        dictionary_key: Key,
    ) -> Result<Option<CLValue>, ExecError> {
        let maybe_stored_value = self
            .tracking_copy
            .borrow_mut()
            .read(&dictionary_key)
            .map_err(Into::<ExecError>::into)?;

        if let Some(stored_value) = maybe_stored_value {
            let stored_value = handle_stored_dictionary_value(dictionary_key, stored_value)?;
            let cl_value = CLValue::try_from(stored_value).map_err(ExecError::TypeMismatch)?;
            Ok(Some(cl_value))
        } else {
            Ok(None)
        }
    }

    /// Puts a dictionary item key from a dictionary referenced by a `uref`.
    pub fn dictionary_put(
        &mut self,
        seed_uref: URef,
        dictionary_item_key: &str,
        cl_value: CLValue,
    ) -> Result<(), ExecError> {
        let dictionary_item_key_bytes = dictionary_item_key.as_bytes();

        if dictionary_item_key_bytes.len() > DICTIONARY_ITEM_KEY_MAX_LENGTH {
            return Err(ExecError::DictionaryItemKeyExceedsLength);
        }

        self.validate_writeable(&seed_uref.into())?;
        self.validate_uref(&seed_uref)?;

        self.validate_cl_value(&cl_value)?;

        let wrapped_cl_value = {
            let dictionary_value = CLValueDictionary::new(
                cl_value,
                seed_uref.addr().to_vec(),
                dictionary_item_key_bytes.to_vec(),
            );
            CLValue::from_t(dictionary_value).map_err(ExecError::from)?
        };

        let dictionary_key = Key::dictionary(seed_uref, dictionary_item_key_bytes);
        self.metered_write_gs_unsafe(dictionary_key, wrapped_cl_value)?;
        Ok(())
    }

    /// Gets system contract by name.
    pub(crate) fn get_system_contract(
        &self,
        name: &str,
    ) -> Result<AddressableEntityHash, ExecError> {
        let registry = self.system_entity_registry()?;
        let hash = registry.get(name).ok_or_else(|| {
            error!("Missing system contract hash: {}", name);
            ExecError::MissingSystemContractHash(name.to_string())
        })?;
        Ok(AddressableEntityHash::new(*hash))
    }

    pub(crate) fn get_system_entity_key(&self, name: &str) -> Result<Key, ExecError> {
        let system_entity_hash = self.get_system_contract(name)?;
        Ok(Key::addressable_entity_key(
            EntityKindTag::System,
            system_entity_hash,
        ))
    }

    /// Returns system entity registry by querying the global state.
    pub fn system_entity_registry(&self) -> Result<SystemHashRegistry, ExecError> {
        self.tracking_copy
            .borrow_mut()
            .get_system_entity_registry()
            .map_err(|err| {
                error!("Missing system entity registry");
                ExecError::TrackingCopy(err)
            })
    }

    pub(super) fn remaining_spending_limit(&self) -> U512 {
        self.remaining_spending_limit
    }

    /// Subtract spent amount from the main purse spending limit.
    pub(crate) fn subtract_amount_spent(&mut self, amount: U512) -> Option<U512> {
        if let Some(res) = self.remaining_spending_limit.checked_sub(amount) {
            self.remaining_spending_limit = res;
            Some(self.remaining_spending_limit)
        } else {
            error!(
                limit = %self.remaining_spending_limit,
                spent = %amount,
                "exceeded main purse spending limit"
            );
            self.remaining_spending_limit = U512::zero();
            None
        }
    }

    /// Sets a new spending limit.
    /// Should be called after inner context returns - if tokens were spent there, it must count
    /// towards global limit for the whole deploy execution.
    pub(crate) fn set_remaining_spending_limit(&mut self, amount: U512) {
        self.remaining_spending_limit = amount;
    }

    /// Adds new message topic.
    pub(crate) fn add_message_topic(
        &mut self,
        topic_name: &str,
        topic_name_hash: TopicNameHash,
    ) -> Result<Result<(), MessageTopicError>, ExecError> {
        let entity_addr = match self.context_key_to_entity_addr() {
            Ok(entity_addr) => entity_addr,
            Err(error) => return Err(error),
        };

        // Take the addressable entity out of the global state
        {
            let mut message_topics = self
                .tracking_copy
                .borrow_mut()
                .get_message_topics(entity_addr.value())?;

            let max_topics_per_contract = self
                .engine_config
                .wasm_config()
                .messages_limits()
                .max_topics_per_contract();

            if message_topics.len() >= max_topics_per_contract as usize {
                return Ok(Err(MessageTopicError::MaxTopicsExceeded));
            }

            if let Err(e) = message_topics.add_topic(topic_name, topic_name_hash) {
                return Ok(Err(e));
            }
        }

        let topic_key = Key::Message(MessageAddr::new_topic_addr(
            entity_addr.value(),
            topic_name_hash,
        ));
        let block_time = self.block_info.block_time();
        let summary = StoredValue::MessageTopic(MessageTopicSummary::new(
            0,
            block_time,
            topic_name.to_string(),
        ));

        self.metered_write_gs_unsafe(topic_key, summary)?;

        Ok(Ok(()))
    }
}
