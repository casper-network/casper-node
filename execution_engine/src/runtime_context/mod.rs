//! The context of execution of WASM code.
use std::{
    cell::RefCell,
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
    fmt::Debug,
    rc::Rc,
};

use tracing::error;

use crate::{
    engine_state::{
        execution_effect::ExecutionEffect, EngineConfig, ExecutionJournal, SystemContractRegistry,
    },
    execution::{AddressGenerator, Error},
    runtime_context::dictionary::DictionaryValue,
    tracking_copy::{AddResult, TrackingCopy, TrackingCopyExt},
};
use casper_storage::global_state::state::StateReader;
use casper_types::{
    account::AccountHash,
    addressable_entity::{
        ActionType, AddKeyFailure, NamedKeys, RemoveKeyFailure, SetThresholdFailure,
        UpdateKeyFailure, Weight,
    },
    bytesrepr::ToBytes,
    package::ContractPackageKind,
    system::auction::EraInfo,
    AccessRights, AddressableEntity, BlockTime, CLType, CLValue, ContextAccessRights, ContractHash,
    ContractPackageHash, DeployHash, DeployInfo, EntryPointAccess, EntryPointType, Gas,
    GrantedAccess, Key, KeyTag, Package, Phase, ProtocolVersion, PublicKey, RuntimeArgs,
    StoredValue, Transfer, TransferAddr, URef, URefAddr, DICTIONARY_ITEM_KEY_MAX_LENGTH,
    KEY_HASH_LENGTH, U512,
};

pub(crate) mod dictionary;
#[cfg(test)]
mod tests;

/// Number of bytes returned from the `random_bytes` function.
pub const RANDOM_BYTES_COUNT: usize = 32;

/// Validates an entry point access with a special validator callback.
///
/// If the passed `access` object is a `Groups` variant, then this function will return a
/// [`Error::InvalidContext`] if there are no groups specified, as such entry point is uncallable.
/// For each [`URef`] in every group that this `access` object refers to, a validator callback is
/// called. If a validator function returns `false` for any of the `URef` in the set, an
/// [`Error::InvalidContext`] is returned.
///
/// Otherwise, if `access` object is a `Public` variant, then the entry point is considered callable
/// and an unit value is returned.
pub fn validate_group_membership(
    contract_package: &Package,
    access: &EntryPointAccess,
    validator: impl Fn(&URef) -> bool,
) -> Result<(), Error> {
    if let EntryPointAccess::Groups(groups) = access {
        if groups.is_empty() {
            // Exits early in a special case of empty list of groups regardless of the group
            // checking logic below it.
            return Err(Error::InvalidContext);
        }

        let find_result = groups.iter().find(|g| {
            contract_package
                .groups()
                .get(g)
                .and_then(|set| set.iter().find(|u| validator(u)))
                .is_some()
        });

        if find_result.is_none() {
            return Err(Error::InvalidContext);
        }
    }
    Ok(())
}

/// Holds information specific to the deployed contract.
pub struct RuntimeContext<'a, R> {
    // Enables look up of specific uref based on human-readable name
    named_keys: &'a mut NamedKeys,
    // Original account/contract for read only tasks taken before execution
    entity: &'a AddressableEntity,
    // Key pointing to the entity we are currently running
    entity_address: Key,
    authorization_keys: BTreeSet<AccountHash>,
    // Used to check uref is known before use (prevents forging urefs)
    access_rights: ContextAccessRights,
    package_kind: ContractPackageKind,
    account_hash: AccountHash,

    address_generator: Rc<RefCell<AddressGenerator>>,
    tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
    engine_config: EngineConfig,

    blocktime: BlockTime,
    protocol_version: ProtocolVersion,
    deploy_hash: DeployHash,
    phase: Phase,
    args: RuntimeArgs,

    gas_limit: Gas,
    gas_counter: Gas,
    remaining_spending_limit: U512,

    transfers: Vec<TransferAddr>,
    //TODO: Will be removed along with stored session in later PR.
    entry_point_type: EntryPointType,
}

impl<'a, R> RuntimeContext<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<Error>,
{
    /// Creates new runtime context where we don't already have one.
    ///
    /// Where we already have a runtime context, consider using `new_from_self()`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        named_keys: &'a mut NamedKeys,
        entity: &'a AddressableEntity,
        entity_address: Key,
        authorization_keys: BTreeSet<AccountHash>,
        access_rights: ContextAccessRights,
        package_kind: ContractPackageKind,
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
        RuntimeContext {
            tracking_copy,
            entry_point_type,
            named_keys,
            access_rights,
            args: runtime_args,
            entity,
            entity_address,
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
            package_kind,
        }
    }

    /// Creates new runtime context cloning values from self.
    #[allow(clippy::too_many_arguments)]
    pub fn new_from_self(
        &self,
        entity_address: Key,
        entry_point_type: EntryPointType,
        named_keys: &'a mut NamedKeys,
        access_rights: ContextAccessRights,
        runtime_args: RuntimeArgs,
    ) -> Self {
        let entity = self.entity;
        let authorization_keys = self.authorization_keys.clone();
        let account_hash = self.account_hash;
        let package_kind = self.package_kind.clone();

        let address_generator = self.address_generator.clone();
        let tracking_copy = self.state();
        let engine_config = self.engine_config;

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
            entity_address,
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
            package_kind,
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
        self.named_keys.contains_key(name)
    }

    /// Returns an instance of the engine config.
    pub fn engine_config(&self) -> EngineConfig {
        self.engine_config
    }

    /// Returns the package kind associated with the current context.
    pub fn get_package_kind(&self) -> ContractPackageKind {
        self.package_kind.clone()
    }

    /// Returns whether the current context is of the system addressable entity.
    pub fn is_system_account(&self) -> bool {
        if let Some(account_hash) = self.package_kind.maybe_account_hash() {
            return account_hash == PublicKey::System.to_account_hash();
        }
        false
    }

    /// Helper function to avoid duplication in `remove_uref`.
    fn remove_key_from_contract(
        &mut self,
        key: Key,
        mut contract: AddressableEntity,
        name: &str,
    ) -> Result<(), Error> {
        if contract.remove_named_key(name).is_none() {
            return Ok(());
        }

        self.metered_write_gs_unsafe(key, contract)?;
        Ok(())
    }

    /// Remove Key from the `named_keys` map of the current context.
    /// It removes both from the ephemeral map (RuntimeContext::named_keys) but
    /// also persistable map (one that is found in the
    /// TrackingCopy/GlobalState).
    pub fn remove_key(&mut self, name: &str) -> Result<(), Error> {
        match self.get_entity_address() {
            account_hash @ Key::Account(_) => {
                let (contract, contract_key): (AddressableEntity, Key) = {
                    let contract_key = self
                        .read_gs_typed::<CLValue>(&account_hash)?
                        .into_t::<Key>()?;

                    let contract: AddressableEntity = self.read_gs_typed(&contract_key)?;
                    (contract, contract_key)
                };
                self.named_keys.remove(name);
                self.remove_key_from_contract(contract_key, contract, name)
            }
            contract_uref @ Key::URef(_) => {
                let contract: AddressableEntity = {
                    let value: StoredValue = self
                        .tracking_copy
                        .borrow_mut()
                        .read(&contract_uref)
                        .map_err(Into::into)?
                        .ok_or(Error::KeyNotFound(contract_uref))?;

                    value.try_into().map_err(Error::TypeMismatch)?
                };

                self.named_keys.remove(name);
                self.remove_key_from_contract(contract_uref, contract, name)
            }
            contract_hash @ Key::Hash(_) => {
                let contract: AddressableEntity = self.read_gs_typed(&contract_hash)?;
                self.named_keys.remove(name);
                self.remove_key_from_contract(contract_hash, contract, name)
            }
            transfer_addr @ Key::Transfer(_) => {
                let _transfer: Transfer = self.read_gs_typed(&transfer_addr)?;
                self.named_keys.remove(name);
                // Users cannot remove transfers from global state
                Ok(())
            }
            deploy_info_addr @ Key::DeployInfo(_) => {
                let _deploy_info: DeployInfo = self.read_gs_typed(&deploy_info_addr)?;
                self.named_keys.remove(name);
                // Users cannot remove deploy infos from global state
                Ok(())
            }
            era_info_addr @ Key::EraInfo(_) => {
                let _era_info: EraInfo = self.read_gs_typed(&era_info_addr)?;
                self.named_keys.remove(name);
                // Users cannot remove era infos from global state
                Ok(())
            }
            Key::Balance(_) => {
                self.named_keys.remove(name);
                Ok(())
            }
            Key::Bid(_) => {
                self.named_keys.remove(name);
                Ok(())
            }
            Key::Withdraw(_) => {
                self.named_keys.remove(name);
                Ok(())
            }
            Key::Dictionary(_) => {
                self.named_keys.remove(name);
                Ok(())
            }
            Key::SystemContractRegistry => {
                error!("should not remove the system contract registry key");
                Err(Error::RemoveKeyFailure(RemoveKeyFailure::PermissionDenied))
            }
            Key::EraSummary => {
                self.named_keys.remove(name);
                Ok(())
            }
            Key::Unbond(_) => {
                self.named_keys.remove(name);
                Ok(())
            }
            Key::ChainspecRegistry => {
                error!("should not remove the chainspec registry key");
                Err(Error::RemoveKeyFailure(RemoveKeyFailure::PermissionDenied))
            }
            Key::ChecksumRegistry => {
                error!("should not remove the checksum registry key");
                Err(Error::RemoveKeyFailure(RemoveKeyFailure::PermissionDenied))
            }
        }
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

    /// Returns the base key.
    ///
    /// This could be either a [`Key::Account`] or a [`Key::Hash`] depending on the entry point
    /// type.
    pub fn get_entity_address(&self) -> Key {
        self.entity_address
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
        let named_key_value = StoredValue::CLValue(CLValue::from_t((name.clone(), key))?);
        self.validate_value(&named_key_value)?;
        self.metered_add_gs_unsafe(self.get_entity_address(), named_key_value)?;
        self.insert_named_key(name, key);
        Ok(())
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
            .map_err(Into::into)?
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

        let maybe_stored_value = self
            .tracking_copy
            .borrow_mut()
            .read(key)
            .map_err(Into::into)?;

        let stored_value = match maybe_stored_value {
            Some(stored_value) => dictionary::handle_stored_value(*key, stored_value)?,
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

    /// Returns current effects of a tracking copy.
    pub fn effect(&self) -> ExecutionEffect {
        self.tracking_copy.borrow().effect()
    }

    /// Returns an `ExecutionJournal`.
    pub fn execution_journal(&self) -> ExecutionJournal {
        self.tracking_copy.borrow().execution_journal()
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
            StoredValue::ContractWasm(_) => Ok(()),
            StoredValue::Contract(_) => Ok(()),
            StoredValue::AddressableEntity(contract_header) => contract_header
                .named_keys()
                .values()
                .try_for_each(|key| self.validate_key(key)),
            // TODO: anything to validate here?
            StoredValue::ContractPackage(_) => Ok(()),
            StoredValue::Transfer(_) => Ok(()),
            StoredValue::DeployInfo(_) => Ok(()),
            StoredValue::EraInfo(_) => Ok(()),
            StoredValue::Bid(_) => Ok(()),
            StoredValue::Withdraw(_) => Ok(()),
            StoredValue::Unbonding(_) => Ok(()),
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
            Key::Account(_) => true,
            Key::Hash(_) => true,
            Key::URef(uref) => uref.is_readable(),
            Key::Transfer(_) => true,
            Key::DeployInfo(_) => true,
            Key::EraInfo(_) => true,
            Key::Balance(_) => false,
            Key::Bid(_) => true,
            Key::Withdraw(_) => true,
            Key::Dictionary(_) => true,
            Key::SystemContractRegistry => true,
            Key::EraSummary => true,
            Key::Unbond(_) => true,
            Key::ChainspecRegistry => true,
            Key::ChecksumRegistry => true,
        }
    }

    /// Tests whether addition to `key` is valid.
    pub fn is_addable(&self, key: &Key) -> bool {
        match key {
            Key::Account(_) => false,
            Key::Hash(_) => &self.get_entity_address() == key, // ???
            Key::URef(uref) => uref.is_addable(),
            Key::Transfer(_) => false,
            Key::DeployInfo(_) => false,
            Key::EraInfo(_) => false,
            Key::Balance(_) => false,
            Key::Bid(_) => false,
            Key::Withdraw(_) => false,
            Key::Dictionary(_) => false,
            Key::SystemContractRegistry => false,
            Key::EraSummary => false,
            Key::Unbond(_) => false,
            Key::ChainspecRegistry => false,
            Key::ChecksumRegistry => false,
        }
    }

    /// Tests whether writing to `key` is valid.
    pub fn is_writeable(&self, key: &Key) -> bool {
        match key {
            Key::Account(_) | Key::Hash(_) => false,
            Key::URef(uref) => uref.is_writeable(),
            Key::Transfer(_) => false,
            Key::DeployInfo(_) => false,
            Key::EraInfo(_) => false,
            Key::Balance(_) => false,
            Key::Bid(_) => false,
            Key::Withdraw(_) => false,
            Key::Dictionary(_) => false,
            Key::SystemContractRegistry => false,
            Key::EraSummary => false,
            Key::Unbond(_) => false,
            Key::ChainspecRegistry => false,
            Key::ChecksumRegistry => false,
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
        let is_system = self.package_kind.is_system();
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

    /// Checks if we are calling a system contract.
    pub(crate) fn is_system_contract(&self, contract_hash: &ContractHash) -> Result<bool, Error> {
        Ok(self
            .system_contract_registry()?
            .has_contract_hash(contract_hash))
    }

    /// Charges gas for specified amount of bytes used.
    fn charge_gas_storage(&mut self, bytes_count: usize) -> Result<(), Error> {
        if let Some(base_key) = self.get_entity_address().into_hash() {
            let contract_hash = ContractHash::new(base_key);
            if self.is_system_contract(&contract_hash)? {
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
        let entity_key = self.get_entity_address_by_account_hash()?;

        // Check permission to modify associated keys
        if !self.is_valid_context(entity_key) {
            // Exit early with error to avoid mutations
            return Err(AddKeyFailure::PermissionDenied.into());
        }

        if !self.entity().can_manage_keys_with(&self.authorization_keys) {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(AddKeyFailure::PermissionDenied.into());
        }

        // Converts an account's public key into a URef
        let key: Key = self.get_entity_address();

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
        let entity_key = self.get_entity_address_by_account_hash()?;
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
        // let key = Key::Account(self.contract().account_hash());
        let key: Key = self.get_entity_address();

        // Take an account out of the global state
        let mut entity: AddressableEntity = self.read_gs_typed(&key)?;

        // Exit early in case of error without updating global state
        entity
            .remove_associated_key(account_hash)
            .map_err(Error::from)?;

        let account_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(key, account_value)?;

        Ok(())
    }

    /// Update associated key.
    pub(crate) fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), Error> {
        let entity_key = self.get_entity_address_by_account_hash()?;
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
        let key: Key = self.get_entity_address();

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

    /// Set threshold of an associated key.
    pub(crate) fn set_action_threshold(
        &mut self,
        action_type: ActionType,
        threshold: Weight,
    ) -> Result<(), Error> {
        let entity_key = self.get_entity_address_by_account_hash()?;
        // Check permission to modify associated keys
        if !self.is_valid_context(entity_key) {
            // Exit early with error to avoid mutations
            return Err(SetThresholdFailure::PermissionDeniedError.into());
        }

        if !self.entity().can_manage_keys_with(&self.authorization_keys) {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(SetThresholdFailure::PermissionDeniedError.into());
        }

        let key: Key = self.get_entity_address();

        // Take an addressable entity out of the global state
        let mut entity: AddressableEntity = self.read_gs_typed(&key)?;

        // Exit early in case of error without updating global state
        entity
            .set_action_threshold(action_type, threshold)
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

    fn get_entity_address_by_account_hash(&mut self) -> Result<Key, Error> {
        let cl_value = self.read_gs_typed::<CLValue>(&Key::Account(self.account_hash))?;
        CLValue::into_t::<Key>(cl_value).map_err(Error::CLValue)
    }

    /// Checks if the account context is valid.
    fn is_valid_context(&self, entity_address: Key) -> bool {
        self.get_entity_address() == entity_address
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
        package_hash: ContractPackageHash,
    ) -> Result<Package, Error> {
        let package_hash_key = Key::from(package_hash);
        self.validate_key(&package_hash_key)?;
        let contract_package: Package = self.read_gs_typed(&Key::from(package_hash))?;
        self.validate_uref(&contract_package.access_key())?;
        Ok(contract_package)
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
            .map_err(Into::into)?;

        if let Some(stored_value) = maybe_stored_value {
            let stored_value = dictionary::handle_stored_value(dictionary_key, stored_value)?;
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
            let dictionary_value = DictionaryValue::new(
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
    pub(crate) fn get_system_contract(&self, name: &str) -> Result<ContractHash, Error> {
        let registry = self.system_contract_registry()?;
        let hash = registry.get(name).ok_or_else(|| {
            error!("Missing system contract hash: {}", name);
            Error::MissingSystemContractHash(name.to_string())
        })?;
        Ok(*hash)
    }

    /// Returns system contract registry by querying the global state.
    pub fn system_contract_registry(&self) -> Result<SystemContractRegistry, Error> {
        self.tracking_copy
            .borrow_mut()
            .get_system_contracts()
            .map_err(|_| {
                error!("Missing system contract registry");
                Error::MissingSystemContractRegistry
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
}
