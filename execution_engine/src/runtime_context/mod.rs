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
    tracking_copy::{AddResult, TrackingCopy, TrackingCopyExt},
    AddressGenerator,
};

use casper_types::{
    account::{Account, AccountHash},
    addressable_entity::{
        ActionType, AddKeyFailure, EntityKind, EntityKindTag, MessageTopicError, NamedKeyAddr,
        NamedKeyValue, NamedKeys, RemoveKeyFailure, SetThresholdFailure, UpdateKeyFailure, Weight,
    },
    bytesrepr::ToBytes,
    contract_messages::{
        Message, MessageAddr, MessageChecksum, MessageTopicSummary, Messages, TopicNameHash,
    },
    execution::Effects,
    handle_stored_dictionary_value,
    system::auction::EraInfo,
    AccessRights, AddressableEntity, AddressableEntityHash, BlockTime, CLType, CLValue,
    CLValueDictionary, ContextAccessRights, DeployHash, EntityAddr, EntryPointType, Gas,
    GrantedAccess, Key, KeyTag, Package, PackageHash, Phase, ProtocolVersion, PublicKey,
    RuntimeArgs, StoredValue, SystemEntityRegistry, Transfer, TransferAddr, URef, URefAddr,
    DICTIONARY_ITEM_KEY_MAX_LENGTH, KEY_HASH_LENGTH, U512,
};

use crate::{engine_state::EngineConfig, execution::Error};

/// Number of bytes returned from the `random_bytes` function.
pub const RANDOM_BYTES_COUNT: usize = 32;

/// Holds information specific to the deployed contract.
pub struct RuntimeContext<'a, R> {
    tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    // Enables look up of specific uref based on human-readable name
    named_keys: &'a mut NamedKeys,
    // Used to check uref is known before use (prevents forging urefs)
    access_rights: ContextAccessRights,
    args: RuntimeArgs,
    authorization_keys: BTreeSet<AccountHash>,
    blocktime: BlockTime,
    deploy_hash: DeployHash,
    gas_limit: Gas,
    gas_counter: Gas,
    address_generator: Rc<RefCell<AddressGenerator>>,
    protocol_version: ProtocolVersion,
    phase: Phase,
    engine_config: EngineConfig,
    //TODO: Will be removed along with stored session in later PR.
    entry_point_type: EntryPointType,
    transfers: Vec<TransferAddr>,
    remaining_spending_limit: U512,

    // Original account/contract for read only tasks taken before execution
    entity: &'a AddressableEntity,
    // Key pointing to the entity we are currently running
    entity_key: Key,
    entity_kind: EntityKind,
    account_hash: AccountHash,
    emit_message_cost: U512,
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
        entity: &'a AddressableEntity,
        entity_key: Key,
        authorization_keys: BTreeSet<AccountHash>,
        access_rights: ContextAccessRights,
        entity_kind: EntityKind,
        account_hash: AccountHash,
        address_generator: Rc<RefCell<AddressGenerator>>,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        engine_config: EngineConfig,
        blocktime: BlockTime,
        protocol_version: ProtocolVersion,
        deploy_hash: DeployHash,
        phase: Phase,
        runtime_args: RuntimeArgs,
        gas_limit: Gas,
        gas_counter: Gas,
        transfers: Vec<TransferAddr>,
        remaining_spending_limit: U512,
        entry_point_type: EntryPointType,
    ) -> Self {
        let emit_message_cost = engine_config
            .wasm_config()
            .take_host_function_costs()
            .emit_message
            .cost()
            .into();
        RuntimeContext {
            tracking_copy,
            entry_point_type,
            named_keys,
            access_rights,
            args: runtime_args,
            entity,
            entity_key,
            authorization_keys,
            account_hash,
            blocktime,
            deploy_hash,
            gas_limit,
            gas_counter,
            address_generator,
            protocol_version,
            phase,
            engine_config,
            transfers,
            remaining_spending_limit,
            entity_kind,
            emit_message_cost,
        }
    }

    /// Creates new runtime context cloning values from self.
    #[allow(clippy::too_many_arguments)]
    pub fn new_from_self(
        &self,
        entity_key: Key,
        entry_point_type: EntryPointType,
        named_keys: &'a mut NamedKeys,
        access_rights: ContextAccessRights,
        runtime_args: RuntimeArgs,
    ) -> Self {
        let entity = self.entity;
        let authorization_keys = self.authorization_keys.clone();
        let account_hash = self.account_hash;
        let entity_kind = self.entity_kind;

        let address_generator = self.address_generator.clone();
        let tracking_copy = self.state();
        let engine_config = self.engine_config.clone();

        let blocktime = self.blocktime;
        let protocol_version = self.protocol_version;
        let deploy_hash = self.deploy_hash;
        let phase = self.phase;

        let gas_limit = self.gas_limit;
        let gas_counter = self.gas_counter;
        let remaining_spending_limit = self.remaining_spending_limit();

        let transfers = self.transfers.clone();

        RuntimeContext {
            tracking_copy,
            entry_point_type,
            named_keys,
            access_rights,
            args: runtime_args,
            entity,
            entity_key,
            authorization_keys,
            account_hash,
            blocktime,
            deploy_hash,
            gas_limit,
            gas_counter,
            address_generator,
            protocol_version,
            phase,
            engine_config,
            transfers,
            remaining_spending_limit,
            entity_kind,
            emit_message_cost: self.emit_message_cost,
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

    /// Returns an instance of the engine config.
    pub fn engine_config(&self) -> &EngineConfig {
        &self.engine_config
    }

    /// Returns the package kind associated with the current context.
    pub fn get_entity_kind(&self) -> EntityKind {
        self.entity_kind
    }

    /// Returns whether the current context is of the system addressable entity.
    pub fn is_system_account(&self) -> bool {
        if let Some(account_hash) = self.entity_kind.maybe_account_hash() {
            return account_hash == PublicKey::System.to_account_hash();
        }
        false
    }

    /// Helper function to avoid duplication in `remove_uref`.
    fn remove_key_from_entity(&mut self, entity_key: Key, name: &str) -> Result<(), Error> {
        let entity_addr = if let Key::AddressableEntity(addr) = entity_key {
            addr
        } else {
            return Err(Error::UnexpectedKeyVariant(entity_key));
        };

        let named_key_addr = NamedKeyAddr::new_from_string(entity_addr, name.to_string())?;
        if let Some(StoredValue::NamedKey(_)) = self.read_gs(&Key::NamedKey(named_key_addr))? {
            self.prune_gs_unsafe(Key::NamedKey(named_key_addr));
        }
        Ok(())
    }

    /// Remove Key from the `named_keys` map of the current context.
    /// It removes both from the ephemeral map (RuntimeContext::named_keys) but
    /// also persistable map (one that is found in the
    /// TrackingCopy/GlobalState).
    pub fn remove_key(&mut self, name: &str) -> Result<(), Error> {
        let entity_key = self.get_entity_key();
        self.named_keys.remove(name);
        self.remove_key_from_entity(entity_key, name)
    }

    /// Returns the block time.
    pub fn get_blocktime(&self) -> BlockTime {
        self.blocktime
    }

    /// Returns the deploy hash.
    pub fn get_deploy_hash(&self) -> DeployHash {
        self.deploy_hash
    }

    /// Extends access rights with a new map.
    pub fn access_rights_extend(&mut self, urefs: &[URef]) {
        self.access_rights.extend(urefs);
    }

    /// Returns a mapping of access rights for each [`URef`]s address.
    pub fn access_rights(&self) -> &ContextAccessRights {
        &self.access_rights
    }

    /// Returns contract of the caller.
    pub fn entity(&self) -> &'a AddressableEntity {
        self.entity
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

    /// Returns the base key of [`Key::AddressableEntity`]
    pub fn get_entity_key(&self) -> Key {
        self.entity_key
    }

    /// Returns the initiater of the call chain.
    pub fn get_caller(&self) -> AccountHash {
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

    /// Generates new deterministic hash for uses as an address.
    pub fn new_hash_address(&mut self) -> Result<[u8; KEY_HASH_LENGTH], Error> {
        Ok(self.address_generator.borrow_mut().new_hash_address())
    }

    /// Returns 32 pseudo random bytes.
    pub fn random_bytes(&mut self) -> Result<[u8; RANDOM_BYTES_COUNT], Error> {
        Ok(self.address_generator.borrow_mut().create_address())
    }

    /// Creates new [`URef`] instance.
    pub fn new_uref(&mut self, value: StoredValue) -> Result<URef, Error> {
        let uref = self
            .address_generator
            .borrow_mut()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.insert_uref(uref);
        self.metered_write_gs(Key::URef(uref), value)?;
        Ok(uref)
    }

    /// Creates a new URef where the value it stores is CLType::Unit.
    pub(crate) fn new_unit_uref(&mut self) -> Result<URef, Error> {
        self.new_uref(StoredValue::CLValue(CLValue::unit()))
    }

    /// Creates a new transfer address using a transfer address generator.
    pub fn new_transfer_addr(&mut self) -> Result<TransferAddr, Error> {
        let transfer_addr = self.address_generator.borrow_mut().create_address();
        Ok(TransferAddr::new(transfer_addr))
    }

    /// Puts `key` to the map of named keys of current context.
    pub fn put_key(&mut self, name: String, key: Key) -> Result<(), Error> {
        // No need to perform actual validation on the base key because an account or contract (i.e.
        // the element stored under `base_key`) is allowed to add new named keys to itself.
        let entity_addr = if let Key::AddressableEntity(entity_addr) = self.get_entity_key() {
            entity_addr
        } else {
            return Err(Error::InvalidContext);
        };
        let named_key_value =
            StoredValue::NamedKey(NamedKeyValue::from_concrete_values(key, name.clone())?);
        self.validate_value(&named_key_value)?;
        let named_key_addr = NamedKeyAddr::new_from_string(entity_addr, name.clone())?;
        self.metered_write_gs_unsafe(Key::NamedKey(named_key_addr), named_key_value)?;
        self.insert_named_key(name, key);
        Ok(())
    }

    pub(crate) fn write_named_keys(
        &mut self,
        entity_addr: EntityAddr,
        named_keys: NamedKeys,
    ) -> Result<(), Error> {
        for (name, key) in named_keys.iter() {
            let named_key_value =
                StoredValue::NamedKey(NamedKeyValue::from_concrete_values(*key, name.clone())?);
            let named_key_addr = NamedKeyAddr::new_from_string(entity_addr, name.clone())?;
            self.metered_write_gs_unsafe(Key::NamedKey(named_key_addr), named_key_value)?;
        }

        Ok(())
    }

    pub(crate) fn get_named_keys(&mut self, entity_key: Key) -> Result<NamedKeys, Error> {
        let entity_addr = if let Key::AddressableEntity(entity_addr) = entity_key {
            entity_addr
        } else {
            return Err(Error::UnexpectedKeyVariant(entity_key));
        };

        self.tracking_copy
            .borrow_mut()
            .get_named_keys(entity_addr)
            .map_err(Into::into)
    }

    #[cfg(test)]
    pub(crate) fn get_entity(&self) -> AddressableEntity {
        self.entity.clone()
    }

    /// Reads the balance of a purse [`URef`].
    ///
    /// Currently address of a purse [`URef`] is also a hash in the [`Key::Hash`] space.
    #[cfg(test)]
    pub(crate) fn read_purse_uref(&mut self, purse_uref: &URef) -> Result<Option<CLValue>, Error> {
        match self
            .tracking_copy
            .borrow_mut()
            .read(&Key::Hash(purse_uref.addr()))
            .map_err(Error::TrackingCopy)?
        {
            Some(stored_value) => Ok(Some(stored_value.try_into().map_err(Error::TypeMismatch)?)),
            None => Ok(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn write_purse_uref(
        &mut self,
        purse_uref: URef,
        cl_value: CLValue,
    ) -> Result<(), Error> {
        self.metered_write_gs_unsafe(Key::Hash(purse_uref.addr()), cl_value)
    }

    /// Read a stored value under a [`Key`].
    pub fn read_gs(&mut self, key: &Key) -> Result<Option<StoredValue>, Error> {
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
    pub fn read_gs_direct(&mut self, key: &Key) -> Result<Option<StoredValue>, Error> {
        self.tracking_copy
            .borrow_mut()
            .read(key)
            .map_err(Into::into)
    }

    /// This method is a wrapper over `read_gs` in the sense that it extracts the type held by a
    /// `StoredValue` stored in the global state in a type safe manner.
    ///
    /// This is useful if you want to get the exact type from global state.
    pub fn read_gs_typed<T>(&mut self, key: &Key) -> Result<T, Error>
    where
        T: TryFrom<StoredValue>,
        T::Error: Debug,
    {
        let value = match self.read_gs(key)? {
            None => return Err(Error::KeyNotFound(*key)),
            Some(value) => value,
        };

        value.try_into().map_err(|error| {
            Error::FunctionNotFound(format!(
                "Type mismatch for value under {:?}: {:?}",
                key, error
            ))
        })
    }

    /// Returns all keys based on the tag prefix.
    pub fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, Error> {
        self.tracking_copy
            .borrow_mut()
            .get_keys(key_tag)
            .map_err(Into::into)
    }

    /// Returns all key's that start with prefix, if any.
    pub fn get_keys_with_prefix(&mut self, prefix: &[u8]) -> Result<Vec<Key>, Error> {
        self.tracking_copy
            .borrow_mut()
            .reader()
            .keys_with_prefix(prefix)
            .map_err(Into::into)
    }

    /// Write a transfer instance to the global state.
    pub fn write_transfer(&mut self, key: Key, value: Transfer) {
        if let Key::Transfer(_) = key {
            // Writing a `Transfer` will not exceed write size limit.
            self.tracking_copy
                .borrow_mut()
                .write(key, StoredValue::Transfer(value));
        } else {
            panic!("Do not use this function for writing non-transfer keys")
        }
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
    fn account_to_validated_value(&self, account: Account) -> Result<StoredValue, Error> {
        let value = StoredValue::Account(account);
        self.validate_value(&value)?;
        Ok(value)
    }

    /// Write an account to the global state.
    pub fn write_account(&mut self, key: Key, account: Account) -> Result<(), Error> {
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
    pub fn read_account(&mut self, key: &Key) -> Result<Option<StoredValue>, Error> {
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

    /// Returns the cost charged for the last emitted message.
    pub fn emit_message_cost(&self) -> U512 {
        self.emit_message_cost
    }

    /// Sets the cost charged for the last emitted message.
    pub fn set_emit_message_cost(&mut self, cost: U512) {
        self.emit_message_cost = cost
    }

    /// Returns list of transfers.
    pub fn transfers(&self) -> &Vec<TransferAddr> {
        &self.transfers
    }

    /// Returns mutable list of transfers.
    pub fn transfers_mut(&mut self) -> &mut Vec<TransferAddr> {
        &mut self.transfers
    }

    fn validate_cl_value(&self, cl_value: &CLValue) -> Result<(), Error> {
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
                let key: Key = cl_value.to_owned().into_t()?; // TODO: optimize?
                self.validate_key(&key)
            }
            CLType::URef => {
                let uref: URef = cl_value.to_owned().into_t()?; // TODO: optimize?
                self.validate_uref(&uref)
            }
            tuple @ CLType::Tuple2(_) if *tuple == casper_types::named_key_type() => {
                let (_name, key): (String, Key) = cl_value.to_owned().into_t()?; // TODO: optimize?
                self.validate_key(&key)
            }
            CLType::Tuple2(_) => Ok(()),
        }
    }

    /// Validates whether keys used in the `value` are not forged.
    fn validate_value(&self, value: &StoredValue) -> Result<(), Error> {
        match value {
            StoredValue::CLValue(cl_value) => self.validate_cl_value(cl_value),
            StoredValue::Account(_) => Ok(()),
            StoredValue::ByteCode(_) => Ok(()),
            StoredValue::Contract(_) => Ok(()),
            StoredValue::AddressableEntity(_) => Ok(()),
            // TODO: anything to validate here?
            StoredValue::Package(_) => Ok(()),
            StoredValue::Transfer(_) => Ok(()),
            StoredValue::DeployInfo(_) => Ok(()),
            StoredValue::EraInfo(_) => Ok(()),
            StoredValue::Bid(_) => Ok(()),
            StoredValue::BidKind(_) => Ok(()),
            StoredValue::Withdraw(_) => Ok(()),
            StoredValue::Unbonding(_) => Ok(()),
            StoredValue::ContractPackage(_) => Ok(()),
            StoredValue::ContractWasm(_) => Ok(()),
            StoredValue::MessageTopic(_) => Ok(()),
            StoredValue::Message(_) => Ok(()),
            StoredValue::NamedKey(named_key_value) => {
                self.validate_cl_value(named_key_value.get_key_as_cl_value())?;
                self.validate_cl_value(named_key_value.get_name_as_cl_value())
            }
        }
    }

    /// Validates whether key is not forged (whether it can be found in the
    /// `named_keys`) and whether the version of a key that contract wants
    /// to use, has access rights that are less powerful than access rights'
    /// of the key in the `named_keys`.
    pub(crate) fn validate_key(&self, key: &Key) -> Result<(), Error> {
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
    pub(crate) fn validate_uref(&self, uref: &URef) -> Result<(), Error> {
        if self.access_rights.has_access_rights_to_uref(uref) {
            Ok(())
        } else {
            Err(Error::ForgedReference(*uref))
        }
    }

    /// Validates if a [`Key`] refers to a [`URef`] and has a read bit set.
    fn validate_readable(&self, key: &Key) -> Result<(), Error> {
        if self.is_readable(key) {
            Ok(())
        } else {
            Err(Error::InvalidAccess {
                required: AccessRights::READ,
            })
        }
    }

    /// Validates if a [`Key`] refers to a [`URef`] and has a add bit set.
    fn validate_addable(&self, key: &Key) -> Result<(), Error> {
        if self.is_addable(key) {
            Ok(())
        } else {
            Err(Error::InvalidAccess {
                required: AccessRights::ADD,
            })
        }
    }

    /// Validates if a [`Key`] refers to a [`URef`] and has a write bit set.
    fn validate_writeable(&self, key: &Key) -> Result<(), Error> {
        if self.is_writeable(key) {
            Ok(())
        } else {
            Err(Error::InvalidAccess {
                required: AccessRights::WRITE,
            })
        }
    }

    /// Tests whether reading from the `key` is valid.
    pub fn is_readable(&self, key: &Key) -> bool {
        match key {
            Key::URef(uref) => uref.is_readable(),
            Key::Balance(_) => false,
            Key::Account(_)
            | Key::Hash(_)
            | Key::Transfer(_)
            | Key::DeployInfo(_)
            | Key::EraInfo(_)
            | Key::Bid(_)
            | Key::Withdraw(_)
            | Key::Dictionary(_)
            | Key::SystemEntityRegistry
            | Key::EraSummary
            | Key::Unbond(_)
            | Key::ChainspecRegistry
            | Key::ChecksumRegistry
            | Key::BidAddr(_)
            | Key::Package(_)
            | Key::AddressableEntity(..)
            | Key::ByteCode(..)
            | Key::Message(_)
            | Key::NamedKey(_) => true,
        }
    }

    /// Tests whether addition to `key` is valid.
    pub fn is_addable(&self, key: &Key) -> bool {
        match key {
            Key::AddressableEntity(entity_addr) => match self.get_entity_key().into_entity_hash() {
                Some(entity_hash) => entity_hash == AddressableEntityHash::new(entity_addr.value()),
                None => false,
            },
            Key::URef(uref) => uref.is_addable(),
            Key::NamedKey(named_key_addr) => {
                if let Key::AddressableEntity(entity_addr) = self.get_entity_key() {
                    named_key_addr.entity_addr() == entity_addr
                } else {
                    false
                }
            }
            Key::Hash(_)
            | Key::Account(_)
            | Key::Transfer(_)
            | Key::DeployInfo(_)
            | Key::EraInfo(_)
            | Key::Balance(_)
            | Key::Bid(_)
            | Key::Withdraw(_)
            | Key::Dictionary(_)
            | Key::SystemEntityRegistry
            | Key::EraSummary
            | Key::Unbond(_)
            | Key::ChainspecRegistry
            | Key::ChecksumRegistry
            | Key::BidAddr(_)
            | Key::Package(_)
            | Key::ByteCode(..)
            | Key::Message(_) => false,
        }
    }

    /// Tests whether writing to `key` is valid.
    pub fn is_writeable(&self, key: &Key) -> bool {
        match key {
            Key::URef(uref) => uref.is_writeable(),
            Key::NamedKey(named_key_addr) => {
                if let Key::AddressableEntity(entity_addr) = self.get_entity_key() {
                    named_key_addr.entity_addr() == entity_addr
                } else {
                    false
                }
            }
            Key::Account(_)
            | Key::Hash(_)
            | Key::Transfer(_)
            | Key::DeployInfo(_)
            | Key::EraInfo(_)
            | Key::Balance(_)
            | Key::Bid(_)
            | Key::Withdraw(_)
            | Key::Dictionary(_)
            | Key::SystemEntityRegistry
            | Key::EraSummary
            | Key::Unbond(_)
            | Key::ChainspecRegistry
            | Key::ChecksumRegistry
            | Key::BidAddr(_)
            | Key::Package(_)
            | Key::AddressableEntity(..)
            | Key::ByteCode(..)
            | Key::Message(_) => false,
        }
    }

    /// Safely charge the specified amount of gas, up to the available gas limit.
    ///
    /// Returns [`Error::GasLimit`] if gas limit exceeded and `()` if not.
    /// Intuition about the return value sense is to answer the question 'are we
    /// allowed to continue?'
    pub(crate) fn charge_gas(&mut self, gas: Gas) -> Result<(), Error> {
        let prev = self.gas_counter();
        let gas_limit = self.gas_limit();
        let is_system = self.entity_kind.is_system();

        // gas charge overflow protection
        match prev.checked_add(gas.cost(is_system)) {
            None => {
                self.set_gas_counter(gas_limit);
                Err(Error::GasLimit)
            }
            Some(val) if val > gas_limit => {
                self.set_gas_counter(gas_limit);
                Err(Error::GasLimit)
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
        contract_hash: &AddressableEntityHash,
    ) -> Result<bool, Error> {
        Ok(self
            .system_contract_registry()?
            .has_contract_hash(contract_hash))
    }

    /// Charges gas for specified amount of bytes used.
    fn charge_gas_storage(&mut self, bytes_count: usize) -> Result<(), Error> {
        if let Some(base_key) = self.get_entity_key().into_entity_hash_addr() {
            let entity_hash = AddressableEntityHash::new(base_key);
            if self.is_system_addressable_entity(&entity_hash)? {
                // Don't charge storage used while executing a system contract.
                return Ok(());
            }
        }

        let storage_costs = self.engine_config.wasm_config().storage_costs();

        let gas_cost = storage_costs.calculate_gas_cost(bytes_count);

        self.charge_gas(gas_cost)
    }

    /// Charges gas for using a host system contract's entrypoint.
    pub(crate) fn charge_system_contract_call<T>(&mut self, call_cost: T) -> Result<(), Error>
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

    /// Writes data to global state with a measurement.
    ///
    /// Use with caution - there is no validation done as the key is assumed to be validated
    /// already.
    pub(crate) fn metered_write_gs_unsafe<K, V>(&mut self, key: K, value: V) -> Result<(), Error>
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
        topic_value: MessageTopicSummary,
        message_key: Key,
        message_value: MessageChecksum,
        message: Message,
    ) -> Result<(), Error> {
        let topic_value = StoredValue::MessageTopic(topic_value);
        let message_value = StoredValue::Message(message_value);

        // Charge for amount as measured by serialized length
        let bytes_count = topic_value.serialized_length() + message_value.serialized_length();
        self.charge_gas_storage(bytes_count)?;

        self.tracking_copy.borrow_mut().emit_message(
            topic_key,
            topic_value,
            message_key,
            message_value,
            message,
        );
        Ok(())
    }

    /// Writes data to a global state and charges for bytes stored.
    ///
    /// This method performs full validation of the key to be written.
    pub(crate) fn metered_write_gs<T>(&mut self, key: Key, value: T) -> Result<(), Error>
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
    ) -> Result<(), Error> {
        let value_bytes_count = value.serialized_length();
        self.charge_gas_storage(value_bytes_count)?;

        match self.tracking_copy.borrow_mut().add(key, value) {
            Err(storage_error) => Err(storage_error.into()),
            Ok(AddResult::Success) => Ok(()),
            Ok(AddResult::KeyNotFound(key)) => Err(Error::KeyNotFound(key)),
            Ok(AddResult::TypeMismatch(type_mismatch)) => Err(Error::TypeMismatch(type_mismatch)),
            Ok(AddResult::Serialization(error)) => Err(Error::BytesRepr(error)),
            Ok(AddResult::Transform(error)) => Err(Error::Transform(error)),
        }
    }

    /// Adds `value` to the `key`. The premise for being able to `add` value is
    /// that the type of it value can be added (is a Monoid). If the
    /// values can't be added, either because they're not a Monoid or if the
    /// value stored under `key` has different type, then `TypeMismatch`
    /// errors is returned.
    pub(crate) fn metered_add_gs<K, V>(&mut self, key: K, value: V) -> Result<(), Error>
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
    ) -> Result<(), Error> {
        let entity_key = match self.entry_point_type {
            EntryPointType::AddressableEntity => self.entity_key,
            EntryPointType::Session | EntryPointType::Factory => {
                self.get_entity_address_for_account_hash(self.account_hash)?
            }
        };

        // Check permission to modify associated keys
        if !self.is_valid_context(entity_key) {
            // Exit early with error to avoid mutations
            return Err(AddKeyFailure::PermissionDenied.into());
        }

        // Converts an account's public key into a URef
        let key = self.get_entity_key();

        // Take an addressable entity out of the global state
        let entity = {
            let mut entity: AddressableEntity = self.read_gs_typed(&key)?;

            if entity.associated_keys().len() >= (self.engine_config.max_associated_keys() as usize)
            {
                return Err(Error::AddKeyFailure(AddKeyFailure::MaxKeysLimit));
            }

            // Exit early in case of error without updating global state
            entity
                .add_associated_key(account_hash, weight)
                .map_err(Error::from)?;
            entity
        };

        let entity_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(key, entity_value)?;

        Ok(())
    }

    /// Remove associated key.
    pub(crate) fn remove_associated_key(&mut self, account_hash: AccountHash) -> Result<(), Error> {
        let entity_key = self.get_entity_address_for_account_hash(self.account_hash)?;
        // Check permission to modify associated keys
        if !self.is_valid_context(entity_key) {
            // Exit early with error to avoid mutations
            return Err(RemoveKeyFailure::PermissionDenied.into());
        }

        if !self.entity().can_manage_keys_with(&self.authorization_keys) {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(RemoveKeyFailure::PermissionDenied.into());
        }

        // Converts an account's public key into a URef
        let contract_hash = self.get_entity_key();

        // Take an account out of the global state
        let mut entity: AddressableEntity = self.read_gs_typed(&contract_hash)?;

        // Exit early in case of error without updating global state
        entity
            .remove_associated_key(account_hash)
            .map_err(Error::from)?;

        let account_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(contract_hash, account_value)?;

        Ok(())
    }

    /// Update associated key.
    pub(crate) fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), Error> {
        let entity_key = self.get_entity_address_for_account_hash(self.account_hash)?;
        // Check permission to modify associated keys
        if !self.is_valid_context(entity_key) {
            // Exit early with error to avoid mutations
            return Err(UpdateKeyFailure::PermissionDenied.into());
        }

        if !self.entity().can_manage_keys_with(&self.authorization_keys) {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(UpdateKeyFailure::PermissionDenied.into());
        }

        // Converts an account's public key into a URef
        let key = self.get_entity_key();

        // Take an account out of the global state
        let mut entity: AddressableEntity = self.read_gs_typed(&key)?;

        // Exit early in case of error without updating global state
        entity
            .update_associated_key(account_hash, weight)
            .map_err(Error::from)?;

        let entity_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(key, entity_value)?;

        Ok(())
    }

    pub(crate) fn is_authorized_by_admin(&self) -> bool {
        self.engine_config
            .administrative_accounts()
            .intersection(&self.authorization_keys)
            .next()
            .is_some()
    }

    /// Set threshold of an associated key.
    pub(crate) fn set_action_threshold(
        &mut self,
        action_type: ActionType,
        threshold: Weight,
    ) -> Result<(), Error> {
        let entity_key = match self.entry_point_type {
            EntryPointType::AddressableEntity => self.entity_key,
            EntryPointType::Session | EntryPointType::Factory => {
                self.get_entity_address_for_account_hash(self.account_hash)?
            }
        };
        // Check permission to modify associated keys
        if !self.is_valid_context(entity_key) {
            // Exit early with error to avoid mutations
            return Err(SetThresholdFailure::PermissionDeniedError.into());
        }

        let key = self.get_entity_key();

        // Take an addressable entity out of the global state
        let mut entity: AddressableEntity = self.read_gs_typed(&key)?;

        // Exit early in case of error without updating global state
        if self.is_authorized_by_admin() {
            entity.set_action_threshold_unchecked(action_type, threshold)
        } else {
            entity.set_action_threshold(action_type, threshold)
        }
        .map_err(Error::from)?;

        let entity_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(key, entity_value)?;

        Ok(())
    }

    fn addressable_entity_to_validated_value(
        &self,
        entity: AddressableEntity,
    ) -> Result<StoredValue, Error> {
        let value = StoredValue::AddressableEntity(entity);
        self.validate_value(&value)?;
        Ok(value)
    }

    pub(crate) fn get_entity_address_for_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<Key, Error> {
        let cl_value = self.read_gs_typed::<CLValue>(&Key::Account(account_hash))?;
        CLValue::into_t::<Key>(cl_value).map_err(Error::CLValue)
    }

    pub(crate) fn read_addressable_entity_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<Option<AddressableEntity>, Error> {
        match self.read_gs(&Key::Account(account_hash))? {
            Some(StoredValue::CLValue(cl_value)) => {
                let key: Key = cl_value.into_t().map_err(Error::CLValue)?;
                match self.read_gs(&key)? {
                    Some(StoredValue::AddressableEntity(addressable_entity)) => {
                        Ok(Some(addressable_entity))
                    }
                    Some(_other_variant_2) => Err(Error::UnexpectedStoredValueVariant),
                    None => Ok(None),
                }
            }
            Some(_other_variant_1) => Err(Error::UnexpectedStoredValueVariant),
            None => Ok(None),
        }
    }

    /// Checks if the account context is valid.
    fn is_valid_context(&self, entity_key: Key) -> bool {
        self.get_entity_key() == entity_key
    }

    /// Gets main purse id
    pub fn get_main_purse(&mut self) -> Result<URef, Error> {
        let main_purse = self.entity().main_purse();
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
    ) -> Result<Package, Error> {
        let package_hash_key = Key::from(package_hash);
        self.validate_key(&package_hash_key)?;
        let contract_package: Package = self.read_gs_typed(&Key::from(package_hash))?;
        if !self.is_authorized_by_admin() {
            self.validate_uref(&contract_package.access_key())?;
        }
        Ok(contract_package)
    }

    pub(crate) fn get_package(&mut self, package_hash: PackageHash) -> Result<Package, Error> {
        self.tracking_copy
            .borrow_mut()
            .get_package(package_hash)
            .map_err(Into::into)
    }

    pub(crate) fn get_contract_entity(
        &mut self,
        entity_key: Key,
    ) -> Result<(AddressableEntity, bool), Error> {
        let entity_hash = if let Some(entity_hash) = entity_key.into_entity_hash() {
            entity_hash
        } else {
            return Err(Error::UnexpectedKeyVariant(entity_key));
        };

        self.tracking_copy
            .borrow_mut()
            .get_entity(entity_hash)
            .map_err(Into::into)
    }

    /// Gets a dictionary item key from a dictionary referenced by a `uref`.
    pub(crate) fn dictionary_get(
        &mut self,
        uref: URef,
        dictionary_item_key: &str,
    ) -> Result<Option<CLValue>, Error> {
        self.validate_readable(&uref.into())?;
        self.validate_key(&uref.into())?;
        let dictionary_item_key_bytes = dictionary_item_key.as_bytes();

        if dictionary_item_key_bytes.len() > DICTIONARY_ITEM_KEY_MAX_LENGTH {
            return Err(Error::DictionaryItemKeyExceedsLength);
        }

        let dictionary_key = Key::dictionary(uref, dictionary_item_key_bytes);
        self.dictionary_read(dictionary_key)
    }

    /// Gets a dictionary value from a dictionary `Key`.
    pub(crate) fn dictionary_read(
        &mut self,
        dictionary_key: Key,
    ) -> Result<Option<CLValue>, Error> {
        let maybe_stored_value = self
            .tracking_copy
            .borrow_mut()
            .read(&dictionary_key)
            .map_err(Into::<Error>::into)?;

        if let Some(stored_value) = maybe_stored_value {
            let stored_value = handle_stored_dictionary_value(dictionary_key, stored_value)?;
            let cl_value = CLValue::try_from(stored_value).map_err(Error::TypeMismatch)?;
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
    ) -> Result<(), Error> {
        let dictionary_item_key_bytes = dictionary_item_key.as_bytes();

        if dictionary_item_key_bytes.len() > DICTIONARY_ITEM_KEY_MAX_LENGTH {
            return Err(Error::DictionaryItemKeyExceedsLength);
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
            CLValue::from_t(dictionary_value).map_err(Error::from)?
        };

        let dictionary_key = Key::dictionary(seed_uref, dictionary_item_key_bytes);
        self.metered_write_gs_unsafe(dictionary_key, wrapped_cl_value)?;
        Ok(())
    }

    /// Gets system contract by name.
    pub(crate) fn get_system_contract(&self, name: &str) -> Result<AddressableEntityHash, Error> {
        let registry = self.system_contract_registry()?;
        let hash = registry.get(name).ok_or_else(|| {
            error!("Missing system contract hash: {}", name);
            Error::MissingSystemContractHash(name.to_string())
        })?;
        Ok(*hash)
    }

    pub(crate) fn get_system_entity_key(&self, name: &str) -> Result<Key, Error> {
        let system_entity_hash = self.get_system_contract(name)?;
        Ok(Key::addressable_entity_key(
            EntityKindTag::System,
            system_entity_hash,
        ))
    }

    /// Returns system contract registry by querying the global state.
    pub fn system_contract_registry(&self) -> Result<SystemEntityRegistry, Error> {
        self.tracking_copy
            .borrow_mut()
            .get_system_contracts()
            .map_err(|err| {
                error!("Missing system contract registry");
                Error::TrackingCopy(err)
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
    ) -> Result<Result<(), MessageTopicError>, Error> {
        let entity_key: Key = self.get_entity_key();
        let entity_addr = entity_key.into_entity_hash().ok_or(Error::InvalidContext)?;

        // Take the addressable entity out of the global state
        let entity = {
            let mut entity: AddressableEntity = self.read_gs_typed(&entity_key)?;

            let max_topics_per_contract = self
                .engine_config
                .wasm_config()
                .messages_limits()
                .max_topics_per_contract();

            if entity.message_topics().len() >= max_topics_per_contract as usize {
                return Ok(Err(MessageTopicError::MaxTopicsExceeded));
            }

            if let Err(e) = entity.add_message_topic(topic_name, topic_name_hash) {
                return Ok(Err(e));
            }
            entity
        };

        let topic_key = Key::Message(MessageAddr::new_topic_addr(entity_addr, topic_name_hash));
        let summary = StoredValue::MessageTopic(MessageTopicSummary::new(0, self.get_blocktime()));

        let entity_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(entity_key, entity_value)?;
        self.metered_write_gs_unsafe(topic_key, summary)?;

        Ok(Ok(()))
    }
}
