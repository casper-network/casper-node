//! This module contains executor state of the WASM code.
mod args;
mod auction_internal;
mod externals;
mod handle_payment_internal;
mod host_function_flag;
mod mint_internal;
pub mod stack;
mod utils;
mod wasm_prep;

use std::{
    cmp,
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    iter::FromIterator,
};

use casper_wasm::elements::Module;
use casper_wasmi::{MemoryRef, Trap, TrapCode};
use tracing::error;

#[cfg(feature = "test-support")]
use casper_wasmi::RuntimeValue;

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::{auction::Auction, handle_payment::HandlePayment, mint::Mint},
    tracking_copy::TrackingCopyExt,
};
use casper_types::{
    account::{Account, AccountHash},
    addressable_entity::{
        self, ActionThresholds, ActionType, AddKeyFailure, AddressableEntity,
        AddressableEntityHash, AssociatedKeys, EntityKindTag, EntryPoint, EntryPointAccess,
        EntryPointType, EntryPoints, MessageTopics, NamedKeys, Parameter, RemoveKeyFailure,
        SetThresholdFailure, UpdateKeyFailure, Weight, DEFAULT_ENTRY_POINT_NAME,
    },
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contract_messages::{
        Message, MessageAddr, MessageChecksum, MessagePayload, MessageTopicSummary,
    },
    contracts::ContractPackage,
    crypto,
    package::PackageStatus,
    system::{
        self,
        auction::{self, EraInfo},
        handle_payment, mint, CallStackElement, SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT,
        STANDARD_PAYMENT,
    },
    AccessRights, ApiError, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, CLTyped, CLValue,
    ContextAccessRights, ContractWasm, DeployHash, EntityAddr, EntityKind, EntityVersion,
    EntityVersionKey, EntityVersions, Gas, GrantedAccess, Group, Groups, HostFunction,
    HostFunctionCost, Key, NamedArg, Package, PackageHash, Phase, PublicKey, RuntimeArgs,
    StoredValue, Tagged, Transfer, TransferResult, TransferredTo, URef,
    DICTIONARY_ITEM_KEY_MAX_LENGTH, U512,
};

use crate::{
    execution::{self, Error},
    runtime::host_function_flag::HostFunctionFlag,
    runtime_context::RuntimeContext,
};
pub use stack::{RuntimeStack, RuntimeStackFrame, RuntimeStackOverflow};
pub use wasm_prep::{
    PreprocessingError, WasmValidationError, DEFAULT_BR_TABLE_MAX_SIZE, DEFAULT_MAX_GLOBALS,
    DEFAULT_MAX_PARAMETER_COUNT, DEFAULT_MAX_TABLE_SIZE,
};

#[derive(Debug)]
enum CallContractIdentifier {
    Contract {
        contract_hash: AddressableEntityHash,
    },
    ContractPackage {
        contract_package_hash: PackageHash,
        version: Option<EntityVersion>,
    },
}

/// Represents the runtime properties of a WASM execution.
pub struct Runtime<'a, R> {
    context: RuntimeContext<'a, R>,
    memory: Option<MemoryRef>,
    module: Option<Module>,
    host_buffer: Option<CLValue>,
    stack: Option<RuntimeStack>,
    host_function_flag: HostFunctionFlag,
}

impl<'a, R> Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    /// Creates a new runtime instance.
    pub(crate) fn new(context: RuntimeContext<'a, R>) -> Self {
        Runtime {
            context,
            memory: None,
            module: None,
            host_buffer: None,
            stack: None,
            host_function_flag: HostFunctionFlag::default(),
        }
    }

    /// Creates a new runtime instance by cloning the config, and host function flag from `self`.
    fn new_invocation_runtime(
        &self,
        context: RuntimeContext<'a, R>,
        module: Module,
        memory: MemoryRef,
        stack: RuntimeStack,
    ) -> Self {
        Self::check_preconditions(&stack);
        Runtime {
            context,
            memory: Some(memory),
            module: Some(module),
            host_buffer: None,
            stack: Some(stack),
            host_function_flag: self.host_function_flag.clone(),
        }
    }

    /// Creates a new runtime instance with a stack from `self`.
    pub(crate) fn new_with_stack(
        &self,
        context: RuntimeContext<'a, R>,
        stack: RuntimeStack,
    ) -> Self {
        Self::check_preconditions(&stack);
        Runtime {
            context,
            memory: None,
            module: None,
            host_buffer: None,
            stack: Some(stack),
            host_function_flag: self.host_function_flag.clone(),
        }
    }

    /// Preconditions that would render the system inconsistent if violated. Those are strictly
    /// programming errors.
    fn check_preconditions(stack: &RuntimeStack) {
        if stack.is_empty() {
            error!("Call stack should not be empty while creating a new Runtime instance");
            debug_assert!(false);
        }

        if stack.first_frame().unwrap().contract_hash().is_some() {
            error!("First element of the call stack should always represent a Session call");
            debug_assert!(false);
        }
    }

    /// Returns the context.
    pub(crate) fn context(&self) -> &RuntimeContext<'a, R> {
        &self.context
    }

    fn gas(&mut self, amount: Gas) -> Result<(), Error> {
        self.context.charge_gas(amount)
    }

    /// Returns current gas counter.
    fn gas_counter(&self) -> Gas {
        self.context.gas_counter()
    }

    /// Sets new gas counter value.
    fn set_gas_counter(&mut self, new_gas_counter: Gas) {
        self.context.set_gas_counter(new_gas_counter);
    }

    /// Charge for a system contract call.
    ///
    /// This method does not charge for system contract calls if the immediate caller is a system
    /// contract or if we're currently within the scope of a host function call. This avoids
    /// misleading gas charges if one system contract calls other system contract (e.g. auction
    /// contract calls into mint to create new purses).
    pub(crate) fn charge_system_contract_call<T>(&mut self, amount: T) -> Result<(), Error>
    where
        T: Into<Gas>,
    {
        if self.is_system_immediate_caller()? || self.host_function_flag.is_in_host_function_scope()
        {
            return Ok(());
        }

        self.context.charge_system_contract_call(amount)
    }

    fn checked_memory_slice<Ret>(
        &self,
        offset: usize,
        size: usize,
        func: impl FnOnce(&[u8]) -> Ret,
    ) -> Result<Ret, Error> {
        // This is mostly copied from a private function `MemoryInstance::checked_memory_region`
        // that calls a user defined function with a validated slice of memory. This allows
        // usage patterns that does not involve copying data onto heap first i.e. deserialize
        // values without copying data first, etc.
        // NOTE: Depending on the VM backend used in future, this may change, as not all VMs may
        // support direct memory access.
        self.try_get_memory()?
            .with_direct_access(|buffer| {
                let end = offset.checked_add(size).ok_or_else(|| {
                    casper_wasmi::Error::Memory(format!(
                        "trying to access memory block of size {} from offset {}",
                        size, offset
                    ))
                })?;

                if end > buffer.len() {
                    return Err(casper_wasmi::Error::Memory(format!(
                        "trying to access region [{}..{}] in memory [0..{}]",
                        offset,
                        end,
                        buffer.len(),
                    )));
                }

                Ok(func(&buffer[offset..end]))
            })
            .map_err(Into::into)
    }

    /// Returns bytes from the WASM memory instance.
    #[inline]
    fn bytes_from_mem(&self, ptr: u32, size: usize) -> Result<Vec<u8>, Error> {
        self.checked_memory_slice(ptr as usize, size, |data| data.to_vec())
    }

    /// Returns a deserialized type from the WASM memory instance.
    #[inline]
    fn t_from_mem<T: FromBytes>(&self, ptr: u32, size: u32) -> Result<T, Error> {
        let result = self.checked_memory_slice(ptr as usize, size as usize, |data| {
            bytesrepr::deserialize_from_slice(data)
        })?;
        Ok(result?)
    }

    /// Reads key (defined as `key_ptr` and `key_size` tuple) from Wasm memory.
    #[inline]
    fn key_from_mem(&mut self, key_ptr: u32, key_size: u32) -> Result<Key, Error> {
        self.t_from_mem(key_ptr, key_size)
    }

    /// Reads `CLValue` (defined as `cl_value_ptr` and `cl_value_size` tuple) from Wasm memory.
    #[inline]
    fn cl_value_from_mem(
        &mut self,
        cl_value_ptr: u32,
        cl_value_size: u32,
    ) -> Result<CLValue, Error> {
        self.t_from_mem(cl_value_ptr, cl_value_size)
    }

    /// Returns a deserialized string from the WASM memory instance.
    #[inline]
    fn string_from_mem(&self, ptr: u32, size: u32) -> Result<String, Trap> {
        self.t_from_mem(ptr, size).map_err(Trap::from)
    }

    fn get_module_from_entry_points(
        &mut self,
        entry_points: &EntryPoints,
    ) -> Result<Vec<u8>, Error> {
        let module = self.try_get_module()?.clone();
        let entry_point_names: Vec<&str> = entry_points.keys().map(|s| s.as_str()).collect();
        let module_bytes = wasm_prep::get_module_from_entry_points(entry_point_names, module)?;
        Ok(module_bytes)
    }

    #[allow(clippy::wrong_self_convention)]
    fn is_valid_uref(&self, uref_ptr: u32, uref_size: u32) -> Result<bool, Trap> {
        let uref: URef = self.t_from_mem(uref_ptr, uref_size)?;
        Ok(self.context.validate_uref(&uref).is_ok())
    }

    /// Load the uref known by the given name into the Wasm memory
    fn load_key(
        &mut self,
        name_ptr: u32,
        name_size: u32,
        output_ptr: u32,
        output_size: usize,
        bytes_written_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        let name = self.string_from_mem(name_ptr, name_size)?;

        // Get a key and serialize it
        let key = match self.context.named_keys_get(&name) {
            Some(key) => key,
            None => return Ok(Err(ApiError::MissingKey)),
        };

        let key_bytes = match key.to_bytes() {
            Ok(bytes) => bytes,
            Err(error) => return Ok(Err(error.into())),
        };

        // `output_size` has to be greater or equal to the actual length of serialized Key bytes
        if output_size < key_bytes.len() {
            return Ok(Err(ApiError::BufferTooSmall));
        }

        // Set serialized Key bytes into the output buffer
        if let Err(error) = self.try_get_memory()?.set(output_ptr, &key_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        // SAFETY: For all practical purposes following conversion is assumed to be safe
        let bytes_size: u32 = key_bytes
            .len()
            .try_into()
            .expect("Keys should not serialize to many bytes");
        let size_bytes = bytes_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self.try_get_memory()?.set(bytes_written_ptr, &size_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    fn has_key(&mut self, name_ptr: u32, name_size: u32) -> Result<i32, Trap> {
        let name = self.string_from_mem(name_ptr, name_size)?;
        if self.context.named_keys_contains_key(&name) {
            Ok(0)
        } else {
            Ok(1)
        }
    }

    fn put_key(
        &mut self,
        name_ptr: u32,
        name_size: u32,
        key_ptr: u32,
        key_size: u32,
    ) -> Result<(), Trap> {
        let name = self.string_from_mem(name_ptr, name_size)?;
        let key = self.key_from_mem(key_ptr, key_size)?;
        self.context.put_key(name, key).map_err(Into::into)
    }

    fn remove_key(&mut self, name_ptr: u32, name_size: u32) -> Result<(), Trap> {
        let name = self.string_from_mem(name_ptr, name_size)?;
        self.context.remove_key(&name)?;
        Ok(())
    }

    /// Writes runtime context's account main purse to dest_ptr in the Wasm memory.
    fn get_main_purse(&mut self, dest_ptr: u32) -> Result<(), Trap> {
        let purse = self.context.get_main_purse()?;
        let purse_bytes = purse.into_bytes().map_err(Error::BytesRepr)?;
        self.try_get_memory()?
            .set(dest_ptr, &purse_bytes)
            .map_err(|e| Error::Interpreter(e.into()).into())
    }

    /// Writes caller (deploy) account public key to dest_ptr in the Wasm
    /// memory.
    fn get_caller(&mut self, output_size: u32) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }
        let value = CLValue::from_t(self.context.get_caller()).map_err(Error::CLValue)?;
        let value_size = value.inner_bytes().len();

        // Save serialized public key into host buffer
        if let Err(error) = self.write_host_buffer(value) {
            return Ok(Err(error));
        }

        // Write output
        let output_size_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self.try_get_memory()?.set(output_size, &output_size_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }
        Ok(Ok(()))
    }

    /// Gets the immediate caller of the current execution
    fn get_immediate_caller(&self) -> Option<&RuntimeStackFrame> {
        self.stack.as_ref().and_then(|stack| stack.previous_frame())
    }

    /// Checks if immediate caller is of session type of the same account as the provided account
    /// hash.
    fn is_allowed_session_caller(&self, provided_account_hash: &AccountHash) -> bool {
        if self.context.get_caller() == PublicKey::System.to_account_hash() {
            return true;
        }

        if let Some(CallStackElement::Session { account_hash }) = self.get_immediate_caller() {
            return account_hash == provided_account_hash;
        }
        false
    }

    /// Writes runtime context's phase to dest_ptr in the Wasm memory.
    fn get_phase(&mut self, dest_ptr: u32) -> Result<(), Trap> {
        let phase = self.context.phase();
        let bytes = phase.into_bytes().map_err(Error::BytesRepr)?;
        self.try_get_memory()?
            .set(dest_ptr, &bytes)
            .map_err(|e| Error::Interpreter(e.into()).into())
    }

    /// Writes current blocktime to dest_ptr in Wasm memory.
    fn get_blocktime(&self, dest_ptr: u32) -> Result<(), Trap> {
        let blocktime = self
            .context
            .get_blocktime()
            .into_bytes()
            .map_err(Error::BytesRepr)?;
        self.try_get_memory()?
            .set(dest_ptr, &blocktime)
            .map_err(|e| Error::Interpreter(e.into()).into())
    }

    /// Load the uref known by the given name into the Wasm memory
    fn load_call_stack(
        &mut self,
        // (Output) Pointer to number of elements in the call stack.
        call_stack_len_ptr: u32,
        // (Output) Pointer to size in bytes of the serialized call stack.
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }
        let call_stack = match self.try_get_stack() {
            Ok(stack) => stack.call_stack_elements(),
            Err(_error) => return Ok(Err(ApiError::Unhandled)),
        };
        let call_stack_len: u32 = match call_stack.len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };
        let call_stack_len_bytes = call_stack_len.to_le_bytes();

        if let Err(error) = self
            .try_get_memory()?
            .set(call_stack_len_ptr, &call_stack_len_bytes)
        {
            return Err(Error::Interpreter(error.into()).into());
        }

        if call_stack_len == 0 {
            return Ok(Ok(()));
        }

        let call_stack_cl_value = CLValue::from_t(call_stack).map_err(Error::CLValue)?;

        let call_stack_cl_value_bytes_len: u32 =
            match call_stack_cl_value.inner_bytes().len().try_into() {
                Ok(value) => value,
                Err(_) => return Ok(Err(ApiError::OutOfMemory)),
            };

        if let Err(error) = self.write_host_buffer(call_stack_cl_value) {
            return Ok(Err(error));
        }

        let call_stack_cl_value_bytes_len_bytes = call_stack_cl_value_bytes_len.to_le_bytes();

        if let Err(error) = self
            .try_get_memory()?
            .set(result_size_ptr, &call_stack_cl_value_bytes_len_bytes)
        {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Return some bytes from the memory and terminate the current `sub_call`. Note that the return
    /// type is `Trap`, indicating that this function will always kill the current Wasm instance.
    fn ret(&mut self, value_ptr: u32, value_size: usize) -> Trap {
        self.host_buffer = None;

        let mem_get =
            self.checked_memory_slice(value_ptr as usize, value_size, |data| data.to_vec());

        match mem_get {
            Ok(buf) => {
                // Set the result field in the runtime and return the proper element of the `Error`
                // enum indicating that the reason for exiting the module was a call to ret.
                self.host_buffer = bytesrepr::deserialize_from_slice(buf).ok();

                let urefs = match &self.host_buffer {
                    Some(buf) => utils::extract_urefs(buf),
                    None => Ok(vec![]),
                };
                match urefs {
                    Ok(urefs) => {
                        for uref in &urefs {
                            if let Err(error) = self.context.validate_uref(uref) {
                                return Trap::from(error);
                            }
                        }
                        Error::Ret(urefs).into()
                    }
                    Err(e) => e.into(),
                }
            }
            Err(e) => e.into(),
        }
    }

    /// Checks if a [`Key`] is a system contract.
    fn is_system_contract(&self, entity_hash: AddressableEntityHash) -> Result<bool, Error> {
        self.context.is_system_addressable_entity(&entity_hash)
    }

    fn get_named_argument<T: FromBytes + CLTyped>(
        args: &RuntimeArgs,
        name: &str,
    ) -> Result<T, Error> {
        let arg: CLValue = args
            .get(name)
            .cloned()
            .ok_or(Error::Revert(ApiError::MissingArgument))?;
        arg.into_t()
            .map_err(|_| Error::Revert(ApiError::InvalidArgument))
    }

    fn reverter<T: Into<ApiError>>(error: T) -> Error {
        let api_error: ApiError = error.into();
        // NOTE: This is special casing needed to keep the native system contracts propagate
        // GasLimit properly to the user. Once support for wasm system contract will be dropped this
        // won't be necessary anymore.
        match api_error {
            ApiError::Mint(mint_error) if mint_error == mint::Error::GasLimit as u8 => {
                Error::GasLimit
            }
            ApiError::AuctionError(auction_error)
                if auction_error == auction::Error::GasLimit as u8 =>
            {
                Error::GasLimit
            }
            ApiError::HandlePayment(handle_payment_error)
                if handle_payment_error == handle_payment::Error::GasLimit as u8 =>
            {
                Error::GasLimit
            }
            api_error => Error::Revert(api_error),
        }
    }

    /// Calls host mint contract.
    fn call_host_mint(
        &mut self,
        entry_point_name: &str,
        runtime_args: &RuntimeArgs,
        access_rights: ContextAccessRights,
        stack: RuntimeStack,
    ) -> Result<CLValue, Error> {
        let gas_counter = self.gas_counter();

        let mint_hash = self.context.get_system_contract(MINT)?;
        let mint_addr = EntityAddr::new_system_entity_addr(mint_hash.value());

        let mint_named_keys = self
            .context
            .state()
            .borrow_mut()
            .get_named_keys(mint_addr)?;

        let mut named_keys = mint_named_keys;

        let runtime_context = self.context.new_from_self(
            mint_addr.into(),
            EntryPointType::AddressableEntity,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut mint_runtime = self.new_with_stack(runtime_context, stack);

        let engine_config = self.context.engine_config();
        let system_config = engine_config.system_config();
        let mint_costs = system_config.mint_costs();

        let result = match entry_point_name {
            // Type: `fn mint(amount: U512) -> Result<URef, Error>`
            mint::METHOD_MINT => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.mint)?;

                let amount: U512 = Self::get_named_argument(runtime_args, mint::ARG_AMOUNT)?;
                let result: Result<URef, mint::Error> = mint_runtime.mint(amount);
                if let Err(mint::Error::GasLimit) = result {
                    return Err(execution::Error::GasLimit);
                }
                CLValue::from_t(result).map_err(Self::reverter)
            })(),
            mint::METHOD_REDUCE_TOTAL_SUPPLY => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.reduce_total_supply)?;

                let amount: U512 = Self::get_named_argument(runtime_args, mint::ARG_AMOUNT)?;
                let result: Result<(), mint::Error> = mint_runtime.reduce_total_supply(amount);
                CLValue::from_t(result).map_err(Self::reverter)
            })(),
            // Type: `fn create() -> URef`
            mint::METHOD_CREATE => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.create)?;

                let uref = mint_runtime.mint(U512::zero()).map_err(Self::reverter)?;
                CLValue::from_t(uref).map_err(Self::reverter)
            })(),
            // Type: `fn balance(purse: URef) -> Option<U512>`
            mint::METHOD_BALANCE => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.balance)?;

                let uref: URef = Self::get_named_argument(runtime_args, mint::ARG_PURSE)?;
                let maybe_balance: Option<U512> =
                    mint_runtime.balance(uref).map_err(Self::reverter)?;
                CLValue::from_t(maybe_balance).map_err(Self::reverter)
            })(),
            // Type: `fn transfer(maybe_to: Option<AccountHash>, source: URef, target: URef, amount:
            // U512, id: Option<u64>) -> Result<(), Error>`
            mint::METHOD_TRANSFER => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.transfer)?;

                let maybe_to: Option<AccountHash> =
                    Self::get_named_argument(runtime_args, mint::ARG_TO)?;
                let source: URef = Self::get_named_argument(runtime_args, mint::ARG_SOURCE)?;
                let target: URef = Self::get_named_argument(runtime_args, mint::ARG_TARGET)?;
                let amount: U512 = Self::get_named_argument(runtime_args, mint::ARG_AMOUNT)?;
                let id: Option<u64> = Self::get_named_argument(runtime_args, mint::ARG_ID)?;
                let result: Result<(), mint::Error> =
                    mint_runtime.transfer(maybe_to, source, target, amount, id);

                CLValue::from_t(result).map_err(Self::reverter)
            })(),
            // Type: `fn read_base_round_reward() -> Result<U512, Error>`
            mint::METHOD_READ_BASE_ROUND_REWARD => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.read_base_round_reward)?;

                let result: U512 = mint_runtime
                    .read_base_round_reward()
                    .map_err(Self::reverter)?;
                CLValue::from_t(result).map_err(Self::reverter)
            })(),
            mint::METHOD_MINT_INTO_EXISTING_PURSE => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.mint_into_existing_purse)?;

                let amount: U512 = Self::get_named_argument(runtime_args, mint::ARG_AMOUNT)?;
                let existing_purse: URef = Self::get_named_argument(runtime_args, mint::ARG_PURSE)?;

                let result: Result<(), mint::Error> =
                    mint_runtime.mint_into_existing_purse(existing_purse, amount);
                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            _ => CLValue::from_t(()).map_err(Self::reverter),
        };

        // Charge just for the amount that particular entry point cost - using gas cost from the
        // isolated runtime might have a recursive costs whenever system contract calls other system
        // contract.
        self.gas(match mint_runtime.gas_counter().checked_sub(gas_counter) {
            None => gas_counter,
            Some(new_gas) => new_gas,
        })?;

        // Result still contains a result, but the entrypoints logic does not exit early on errors.
        let ret = result?;

        // Update outer spending approved limit.
        self.context
            .set_remaining_spending_limit(mint_runtime.context.remaining_spending_limit());

        let urefs = utils::extract_urefs(&ret)?;
        self.context.access_rights_extend(&urefs);
        {
            let transfers = self.context.transfers_mut();
            *transfers = mint_runtime.context.transfers().to_owned();
        }
        Ok(ret)
    }

    /// Calls host `handle_payment` contract.
    fn call_host_handle_payment(
        &mut self,
        entry_point_name: &str,
        runtime_args: &RuntimeArgs,
        access_rights: ContextAccessRights,
        stack: RuntimeStack,
    ) -> Result<CLValue, Error> {
        let gas_counter = self.gas_counter();

        let handle_payment_hash = self.context.get_system_contract(HANDLE_PAYMENT)?;
        let handle_payment_key =
            Key::addressable_entity_key(EntityKindTag::System, handle_payment_hash);

        let handle_payment_named_keys = self
            .context
            .state()
            .borrow_mut()
            .get_named_keys(EntityAddr::System(handle_payment_hash.value()))?;

        let mut named_keys = handle_payment_named_keys;

        let runtime_context = self.context.new_from_self(
            handle_payment_key,
            EntryPointType::AddressableEntity,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut runtime = self.new_with_stack(runtime_context, stack);

        let engine_config = self.context.engine_config();
        let system_config = engine_config.system_config();
        let handle_payment_costs = system_config.handle_payment_costs();

        let result = match entry_point_name {
            handle_payment::METHOD_GET_PAYMENT_PURSE => (|| {
                runtime.charge_system_contract_call(handle_payment_costs.get_payment_purse)?;

                let rights_controlled_purse =
                    runtime.get_payment_purse().map_err(Self::reverter)?;
                CLValue::from_t(rights_controlled_purse).map_err(Self::reverter)
            })(),
            handle_payment::METHOD_SET_REFUND_PURSE => (|| {
                runtime.charge_system_contract_call(handle_payment_costs.set_refund_purse)?;

                let purse: URef =
                    Self::get_named_argument(runtime_args, handle_payment::ARG_PURSE)?;
                runtime.set_refund_purse(purse).map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),
            handle_payment::METHOD_GET_REFUND_PURSE => (|| {
                runtime.charge_system_contract_call(handle_payment_costs.get_refund_purse)?;

                let maybe_purse = runtime.get_refund_purse().map_err(Self::reverter)?;
                CLValue::from_t(maybe_purse).map_err(Self::reverter)
            })(),
            handle_payment::METHOD_FINALIZE_PAYMENT => (|| {
                runtime.charge_system_contract_call(handle_payment_costs.finalize_payment)?;

                let amount_spent: U512 =
                    Self::get_named_argument(runtime_args, handle_payment::ARG_AMOUNT)?;
                let account: AccountHash =
                    Self::get_named_argument(runtime_args, handle_payment::ARG_ACCOUNT)?;
                let target: URef =
                    Self::get_named_argument(runtime_args, handle_payment::ARG_TARGET)?;
                runtime
                    .finalize_payment(amount_spent, account, target)
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),
            handle_payment::METHOD_DISTRIBUTE_ACCUMULATED_FEES => (|| {
                runtime.charge_system_contract_call(handle_payment_costs.finalize_payment)?;
                runtime
                    .distribute_accumulated_fees()
                    .map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            _ => CLValue::from_t(()).map_err(Self::reverter),
        };

        self.gas(match runtime.gas_counter().checked_sub(gas_counter) {
            None => gas_counter,
            Some(new_gas) => new_gas,
        })?;

        let ret = result?;

        let urefs = utils::extract_urefs(&ret)?;
        self.context.access_rights_extend(&urefs);
        {
            let transfers = self.context.transfers_mut();
            *transfers = runtime.context.transfers().to_owned();
        }
        Ok(ret)
    }

    /// Calls host auction contract.
    fn call_host_auction(
        &mut self,
        entry_point_name: &str,
        runtime_args: &RuntimeArgs,
        access_rights: ContextAccessRights,
        stack: RuntimeStack,
    ) -> Result<CLValue, Error> {
        let gas_counter = self.gas_counter();

        let auction_hash = self.context.get_system_contract(AUCTION)?;
        let auction_key = Key::addressable_entity_key(EntityKindTag::System, auction_hash);

        let auction_named_keys = self
            .context
            .state()
            .borrow_mut()
            .get_named_keys(EntityAddr::System(auction_hash.value()))?;

        let mut named_keys = auction_named_keys;

        let runtime_context = self.context.new_from_self(
            auction_key,
            EntryPointType::AddressableEntity,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut runtime = self.new_with_stack(runtime_context, stack);

        let engine_config = self.context.engine_config();
        let system_config = engine_config.system_config();
        let auction_costs = system_config.auction_costs();

        let result = match entry_point_name {
            auction::METHOD_GET_ERA_VALIDATORS => (|| {
                runtime
                    .context
                    .charge_gas(auction_costs.get_era_validators.into())?;

                let result = runtime.get_era_validators().map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_ADD_BID => (|| {
                runtime.charge_system_contract_call(auction_costs.add_bid)?;

                let account_hash = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
                let delegation_rate =
                    Self::get_named_argument(runtime_args, auction::ARG_DELEGATION_RATE)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

                let result = runtime
                    .add_bid(account_hash, delegation_rate, amount)
                    .map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_WITHDRAW_BID => (|| {
                runtime.charge_system_contract_call(auction_costs.withdraw_bid)?;

                let account_hash = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

                let result = runtime
                    .withdraw_bid(account_hash, amount)
                    .map_err(Self::reverter)?;
                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_DELEGATE => (|| {
                runtime.charge_system_contract_call(auction_costs.delegate)?;

                let delegator = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

                let max_delegators_per_validator =
                    self.context.engine_config().max_delegators_per_validator();
                let minimum_delegation_amount =
                    self.context.engine_config().minimum_delegation_amount();

                let result = runtime
                    .delegate(
                        delegator,
                        validator,
                        amount,
                        max_delegators_per_validator,
                        minimum_delegation_amount,
                    )
                    .map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_UNDELEGATE => (|| {
                runtime.charge_system_contract_call(auction_costs.undelegate)?;

                let delegator = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

                let result = runtime
                    .undelegate(delegator, validator, amount)
                    .map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_REDELEGATE => (|| {
                runtime.charge_system_contract_call(auction_costs.redelegate)?;

                let delegator = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
                let new_validator =
                    Self::get_named_argument(runtime_args, auction::ARG_NEW_VALIDATOR)?;

                let minimum_delegation_amount =
                    self.context.engine_config().minimum_delegation_amount();

                let result = runtime
                    .redelegate(
                        delegator,
                        validator,
                        amount,
                        new_validator,
                        minimum_delegation_amount,
                    )
                    .map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_RUN_AUCTION => (|| {
                runtime.charge_system_contract_call(auction_costs.run_auction)?;

                let era_end_timestamp_millis =
                    Self::get_named_argument(runtime_args, auction::ARG_ERA_END_TIMESTAMP_MILLIS)?;
                let evicted_validators =
                    Self::get_named_argument(runtime_args, auction::ARG_EVICTED_VALIDATORS)?;

                let max_delegators_per_validator =
                    self.context.engine_config().max_delegators_per_validator();
                let minimum_delegation_amount =
                    self.context.engine_config().minimum_delegation_amount();

                runtime
                    .run_auction(
                        era_end_timestamp_millis,
                        evicted_validators,
                        max_delegators_per_validator,
                        minimum_delegation_amount,
                    )
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn slash(validator_account_hashes: &[AccountHash]) -> Result<(), Error>`
            auction::METHOD_SLASH => (|| {
                runtime.context.charge_gas(auction_costs.slash.into())?;

                let validator_public_keys =
                    Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR_PUBLIC_KEYS)?;
                runtime
                    .slash(validator_public_keys)
                    .map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn distribute(reward_factors: BTreeMap<PublicKey, u64>) -> Result<(), Error>`
            auction::METHOD_DISTRIBUTE => (|| {
                runtime
                    .context
                    .charge_gas(auction_costs.distribute.into())?;
                let rewards = Self::get_named_argument(runtime_args, auction::ARG_REWARDS_MAP)?;
                runtime.distribute(rewards).map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn read_era_id() -> Result<EraId, Error>`
            auction::METHOD_READ_ERA_ID => (|| {
                runtime
                    .context
                    .charge_gas(auction_costs.read_era_id.into())?;

                let result = runtime.read_era_id().map_err(Self::reverter)?;
                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_ACTIVATE_BID => (|| {
                runtime.charge_system_contract_call(auction_costs.activate_bid)?;

                let validator_public_key: PublicKey =
                    Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR_PUBLIC_KEY)?;

                runtime
                    .activate_bid(validator_public_key)
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            _ => CLValue::from_t(()).map_err(Self::reverter),
        };

        // Charge for the gas spent during execution in an isolated runtime.
        self.gas(match runtime.gas_counter().checked_sub(gas_counter) {
            None => gas_counter,
            Some(new_gas) => new_gas,
        })?;

        // Result still contains a result, but the entrypoints logic does not exit early on errors.
        let ret = result?;

        let urefs = utils::extract_urefs(&ret)?;
        self.context.access_rights_extend(&urefs);
        {
            let transfers = self.context.transfers_mut();
            *transfers = runtime.context.transfers().to_owned();
        }

        Ok(ret)
    }

    /// Call a contract by pushing a stack element onto the frame.
    pub(crate) fn call_contract_with_stack(
        &mut self,
        contract_hash: AddressableEntityHash,
        entry_point_name: &str,
        args: RuntimeArgs,
        stack: RuntimeStack,
    ) -> Result<CLValue, Error> {
        self.stack = Some(stack);

        self.call_contract(contract_hash, entry_point_name, args)
    }

    pub(crate) fn execute_module_bytes(
        &mut self,
        module_bytes: &Bytes,
        stack: RuntimeStack,
    ) -> Result<CLValue, Error> {
        let protocol_version = self.context.protocol_version();
        let engine_config = self.context.engine_config();
        let wasm_config = engine_config.wasm_config();
        #[cfg(feature = "test-support")]
        let max_stack_height = wasm_config.max_stack_height;
        let module = wasm_prep::preprocess(*wasm_config, module_bytes)?;
        let (instance, memory) =
            utils::instance_and_memory(module.clone(), protocol_version, engine_config)?;
        self.memory = Some(memory);
        self.module = Some(module);
        self.stack = Some(stack);
        self.context.set_args(utils::attenuate_uref_in_args(
            self.context.args().clone(),
            self.context.entity().main_purse().addr(),
            AccessRights::WRITE,
        )?);

        let result = instance.invoke_export(DEFAULT_ENTRY_POINT_NAME, &[], self);

        let error = match result {
            Err(error) => error,
            // If `Ok` and the `host_buffer` is `None`, the contract's execution succeeded but did
            // not explicitly call `runtime::ret()`.  Treat as though the execution
            // returned the unit type `()` as per Rust functions which don't specify a
            // return value.
            Ok(_) => {
                return Ok(self.take_host_buffer().unwrap_or(CLValue::from_t(())?));
            }
        };

        #[cfg(feature = "test-support")]
        dump_runtime_stack_info(instance, max_stack_height);

        if let Some(host_error) = error.as_host_error() {
            // If the "error" was in fact a trap caused by calling `ret` then
            // this is normal operation and we should return the value captured
            // in the Runtime result field.
            let downcasted_error = host_error.downcast_ref::<Error>();
            match downcasted_error {
                Some(Error::Ret(ref _ret_urefs)) => {
                    return self.take_host_buffer().ok_or(Error::ExpectedReturnValue);
                }
                Some(error) => return Err(error.clone()),
                None => return Err(Error::Interpreter(host_error.to_string())),
            }
        }
        Err(Error::Interpreter(error.into()))
    }

    /// Calls contract living under a `key`, with supplied `args`.
    pub fn call_contract(
        &mut self,
        contract_hash: AddressableEntityHash,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Result<CLValue, Error> {
        let identifier = CallContractIdentifier::Contract { contract_hash };

        self.execute_contract(identifier, entry_point_name, args)
    }

    /// Calls `version` of the contract living at `key`, invoking `method` with
    /// supplied `args`. This function also checks the args conform with the
    /// types given in the contract header.
    pub fn call_versioned_contract(
        &mut self,
        contract_package_hash: PackageHash,
        contract_version: Option<EntityVersion>,
        entry_point_name: String,
        args: RuntimeArgs,
    ) -> Result<CLValue, Error> {
        let identifier = CallContractIdentifier::ContractPackage {
            contract_package_hash,
            version: contract_version,
        };

        self.execute_contract(identifier, &entry_point_name, args)
    }

    fn get_context_key_for_contract_call(
        &self,
        entity_hash: AddressableEntityHash,
        entry_point: &EntryPoint,
    ) -> Result<AddressableEntityHash, Error> {
        let current = self.context.entry_point_type();
        let next = entry_point.entry_point_type();
        match (current, next) {
            (EntryPointType::AddressableEntity, EntryPointType::Session) => {
                // Session code can't be called from Contract code for security reasons.
                Err(Error::InvalidContext)
            }
            (EntryPointType::Factory, EntryPointType::Session) => {
                // Session code can't be called from Installer code for security reasons.
                Err(Error::InvalidContext)
            }
            (EntryPointType::Session, EntryPointType::Session) => {
                // Session code called from session reuses current base key
                match self.context.get_entity_key().into_entity_hash() {
                    Some(entity_hash) => Ok(entity_hash),
                    None => Err(Error::InvalidEntity(entity_hash)),
                }
            }
            (EntryPointType::Session, EntryPointType::AddressableEntity)
            | (EntryPointType::AddressableEntity, EntryPointType::AddressableEntity) => {
                Ok(entity_hash)
            }
            _ => {
                // Any other combination (installer, normal, etc.) is a contract context.
                Ok(entity_hash)
            }
        }
    }

    fn try_get_memory(&self) -> Result<&MemoryRef, Error> {
        self.memory.as_ref().ok_or(Error::WasmPreprocessing(
            PreprocessingError::MissingMemorySection,
        ))
    }

    fn try_get_module(&self) -> Result<&Module, Error> {
        self.module
            .as_ref()
            .ok_or(Error::WasmPreprocessing(PreprocessingError::MissingModule))
    }

    fn try_get_stack(&self) -> Result<&RuntimeStack, Error> {
        self.stack.as_ref().ok_or(Error::MissingRuntimeStack)
    }

    fn execute_contract(
        &mut self,
        identifier: CallContractIdentifier,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Result<CLValue, Error> {
        let (entity, entity_hash, package) = match identifier {
            CallContractIdentifier::Contract {
                contract_hash: entity_hash,
            } => {
                let entity_key = if self.context.is_system_addressable_entity(&entity_hash)? {
                    Key::addressable_entity_key(EntityKindTag::System, entity_hash)
                } else {
                    Key::contract_entity_key(entity_hash)
                };

                let entity = if let Some(StoredValue::AddressableEntity(entity)) =
                    self.context.read_gs(&entity_key)?
                {
                    entity
                } else {
                    self.migrate_contract_and_contract_package(entity_hash)?
                };

                let package = Key::from(entity.package_hash());

                let package: Package = self.context.read_gs_typed(&package)?;

                // System contract hashes are disabled at upgrade point
                let is_calling_system_contract = self.is_system_contract(entity_hash)?;

                // Check if provided contract hash is disabled
                let is_contract_enabled = package.is_entity_enabled(&entity_hash);

                if !is_calling_system_contract && !is_contract_enabled {
                    return Err(Error::DisabledEntity(entity_hash));
                }

                (entity, entity_hash, package)
            }
            CallContractIdentifier::ContractPackage {
                contract_package_hash,
                version,
            } => {
                let package = self.context.get_package(contract_package_hash)?;

                let contract_version_key = match version {
                    Some(version) => EntityVersionKey::new(
                        self.context.protocol_version().value().major,
                        version,
                    ),
                    None => match package.current_entity_version() {
                        Some(v) => v,
                        None => {
                            return Err(Error::NoActiveEntityVersions(contract_package_hash));
                        }
                    },
                };
                let entity_hash = package
                    .lookup_entity_hash(contract_version_key)
                    .copied()
                    .ok_or(Error::InvalidEntityVersion(contract_version_key))?;

                let entity_key = if self.context.is_system_addressable_entity(&entity_hash)? {
                    Key::addressable_entity_key(EntityKindTag::System, entity_hash)
                } else {
                    Key::contract_entity_key(entity_hash)
                };

                let entity = if let Some(StoredValue::AddressableEntity(entity)) =
                    self.context.read_gs(&entity_key)?
                {
                    entity
                } else {
                    self.migrate_contract_and_contract_package(entity_hash)?
                };

                (entity, entity_hash, package)
            }
        };

        if let EntityKind::Account(_) = entity.entity_kind() {
            return Err(Error::InvalidContext);
        }

        let protocol_version = self.context.protocol_version();

        // Check for major version compatibility before calling
        if !entity.is_compatible_protocol_version(protocol_version) {
            return Err(Error::IncompatibleProtocolMajorVersion {
                actual: entity.protocol_version().value().major,
                expected: protocol_version.value().major,
            });
        }

        let entry_point = entity
            .entry_point(entry_point_name)
            .cloned()
            .ok_or_else(|| Error::NoSuchMethod(entry_point_name.to_owned()))?;

        let entry_point_type = entry_point.entry_point_type();

        if entry_point_type.is_invalid_context() {
            return Err(Error::InvalidContext);
        }

        // Get contract entry point hash
        // if public, allowed
        // if not public, restricted to user group access
        // if abstract, not allowed
        self.validate_entry_point_access(&package, entry_point_name, entry_point.access())?;

        if self.context.engine_config().strict_argument_checking() {
            let entry_point_args_lookup: BTreeMap<&str, &Parameter> = entry_point
                .args()
                .iter()
                .map(|param| (param.name(), param))
                .collect();

            let args_lookup: BTreeMap<&str, &NamedArg> = args
                .named_args()
                .map(|named_arg| (named_arg.name(), named_arg))
                .collect();

            // variable ensure args type(s) match defined args of entry point
            for (param_name, param) in entry_point_args_lookup {
                if let Some(named_arg) = args_lookup.get(param_name) {
                    if param.cl_type() != named_arg.cl_value().cl_type() {
                        return Err(Error::type_mismatch(
                            param.cl_type().clone(),
                            named_arg.cl_value().cl_type().clone(),
                        ));
                    }
                } else if !param.cl_type().is_option() {
                    return Err(Error::MissingArgument {
                        name: param.name().to_string(),
                    });
                }
            }
        }

        if !self
            .context
            .engine_config()
            .administrative_accounts()
            .is_empty()
            && !package.is_entity_enabled(&entity_hash)
            && !self.context.is_system_addressable_entity(&entity_hash)?
        {
            return Err(Error::DisabledEntity(entity_hash));
        }

        // if session the caller's context
        // else the called contract's context
        let context_entity_hash =
            self.get_context_key_for_contract_call(entity_hash, &entry_point)?;

        let (should_attenuate_urefs, should_validate_urefs) = {
            // Determines if this call originated from the system account based on a first
            // element of the call stack.
            let is_system_account =
                self.context.get_caller() == PublicKey::System.to_account_hash();
            // Is the immediate caller a system contract, such as when the auction calls the mint.
            let is_caller_system_contract =
                self.is_system_contract(self.context.access_rights().context_key())?;
            // Checks if the contract we're about to call is a system contract.
            let is_calling_system_contract = self.is_system_contract(context_entity_hash)?;
            // uref attenuation is necessary in the following circumstances:
            //   the originating account (aka the caller) is not the system account and
            //   the immediate caller is either a normal account or a normal contract and
            //   the target contract about to be called is a normal contract
            let should_attenuate_urefs =
                !is_system_account && !is_caller_system_contract && !is_calling_system_contract;
            let should_validate_urefs = !is_caller_system_contract || !is_calling_system_contract;
            (should_attenuate_urefs, should_validate_urefs)
        };
        let runtime_args = if should_attenuate_urefs {
            // Main purse URefs should be attenuated only when a non-system contract is executed by
            // a non-system account to avoid possible phishing attack scenarios.
            utils::attenuate_uref_in_args(
                args,
                self.context.entity().main_purse().addr(),
                AccessRights::WRITE,
            )?
        } else {
            args
        };

        let extended_access_rights = {
            let mut all_urefs = vec![];
            for arg in runtime_args.to_values() {
                let urefs = utils::extract_urefs(arg)?;
                if should_validate_urefs {
                    for uref in &urefs {
                        self.context.validate_uref(uref)?;
                    }
                }
                all_urefs.extend(urefs);
            }
            all_urefs
        };

        let entity_addr = EntityAddr::new_with_tag(entity.entity_kind(), entity_hash.value());

        let entity_named_keys = self
            .context
            .state()
            .borrow_mut()
            .get_named_keys(entity_addr)?;

        let access_rights = {
            let mut access_rights = entity.extract_access_rights(entity_hash, &entity_named_keys);
            access_rights.extend(&extended_access_rights);
            access_rights
        };

        let stack = {
            let mut stack = self.try_get_stack()?.clone();

            stack.push(CallStackElement::stored_contract(
                entity.package_hash(),
                entity_hash,
            ))?;

            stack
        };

        if let EntityKind::System(system_contract_type) = entity.entity_kind() {
            let entry_point_name = entry_point.name();

            match system_contract_type {
                SystemEntityType::Mint => {
                    return self.call_host_mint(
                        entry_point_name,
                        &runtime_args,
                        access_rights,
                        stack,
                    );
                }
                SystemEntityType::HandlePayment => {
                    return self.call_host_handle_payment(
                        entry_point_name,
                        &runtime_args,
                        access_rights,
                        stack,
                    );
                }
                SystemEntityType::Auction => {
                    return self.call_host_auction(
                        entry_point_name,
                        &runtime_args,
                        access_rights,
                        stack,
                    );
                }
                // Not callable
                SystemEntityType::StandardPayment => {}
            }
        }

        let module: Module = {
            let byte_code_addr = entity.byte_code_addr();

            let byte_code_key = match entity.entity_kind() {
                EntityKind::System(_) | EntityKind::Account(_) => {
                    Key::ByteCode(ByteCodeAddr::Empty)
                }
                EntityKind::SmartContract => {
                    Key::ByteCode(ByteCodeAddr::new_wasm_addr(byte_code_addr))
                }
            };

            let byte_code: ByteCode = match self.context.read_gs(&byte_code_key)? {
                Some(StoredValue::ByteCode(byte_code)) => byte_code,
                Some(_) => return Err(Error::InvalidByteCode(entity.byte_code_hash())),
                None => return Err(Error::KeyNotFound(byte_code_key)),
            };

            casper_wasm::deserialize_buffer(byte_code.bytes())?
        };

        let entity_tag = entity.entity_kind().tag();

        let mut named_keys = entity_named_keys;

        let context_entity_key = Key::addressable_entity_key(entity_tag, entity_hash);

        let context = self.context.new_from_self(
            context_entity_key,
            entry_point.entry_point_type(),
            &mut named_keys,
            access_rights,
            runtime_args,
        );

        let (instance, memory) = utils::instance_and_memory(
            module.clone(),
            self.context.protocol_version(),
            self.context.engine_config(),
        )?;
        let runtime = &mut Runtime::new_invocation_runtime(self, context, module, memory, stack);
        let result = instance.invoke_export(entry_point.name(), &[], runtime);
        // The `runtime`'s context was initialized with our counter from before the call and any gas
        // charged by the sub-call was added to its counter - so let's copy the correct value of the
        // counter from there to our counter. Do the same for the message cost tracking.
        self.context.set_gas_counter(runtime.context.gas_counter());
        self.context
            .set_emit_message_cost(runtime.context.emit_message_cost());
        let transfers = self.context.transfers_mut();
        *transfers = runtime.context.transfers().to_owned();

        return match result {
            Ok(_) => {
                // If `Ok` and the `host_buffer` is `None`, the contract's execution succeeded but
                // did not explicitly call `runtime::ret()`.  Treat as though the
                // execution returned the unit type `()` as per Rust functions which
                // don't specify a return value.
                self.context
                    .set_remaining_spending_limit(runtime.context.remaining_spending_limit());
                Ok(runtime.take_host_buffer().unwrap_or(CLValue::from_t(())?))
            }
            Err(error) => {
                #[cfg(feature = "test-support")]
                dump_runtime_stack_info(
                    instance,
                    self.context.engine_config().wasm_config().max_stack_height,
                );
                if let Some(host_error) = error.as_host_error() {
                    // If the "error" was in fact a trap caused by calling `ret` then this is normal
                    // operation and we should return the value captured in the Runtime result
                    // field.
                    let downcasted_error = host_error.downcast_ref::<Error>();
                    match downcasted_error {
                        Some(Error::Ret(ref ret_urefs)) => {
                            // Insert extra urefs returned from call.
                            // Those returned URef's are guaranteed to be valid as they were already
                            // validated in the `ret` call inside context we ret from.
                            self.context.access_rights_extend(ret_urefs);

                            // Stored contracts are expected to always call a `ret` function,
                            // otherwise it's an error.
                            return runtime.take_host_buffer().ok_or(Error::ExpectedReturnValue);
                        }
                        Some(error) => return Err(error.clone()),
                        None => return Err(Error::Interpreter(host_error.to_string())),
                    }
                }
                Err(Error::Interpreter(error.into()))
            }
        };
    }

    fn call_contract_host_buffer(
        &mut self,
        contract_hash: AddressableEntityHash,
        entry_point_name: &str,
        args_bytes: &[u8],
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        // Exit early if the host buffer is already occupied
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        let args: RuntimeArgs = bytesrepr::deserialize_from_slice(args_bytes)?;
        let result = self.call_contract(contract_hash, entry_point_name, args)?;
        self.manage_call_contract_host_buffer(result_size_ptr, result)
    }

    fn call_versioned_contract_host_buffer(
        &mut self,
        contract_package_hash: PackageHash,
        contract_version: Option<EntityVersion>,
        entry_point_name: String,
        args_bytes: &[u8],
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        // Exit early if the host buffer is already occupied
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        let args: RuntimeArgs = bytesrepr::deserialize_from_slice(args_bytes)?;
        let result = self.call_versioned_contract(
            contract_package_hash,
            contract_version,
            entry_point_name,
            args,
        )?;
        self.manage_call_contract_host_buffer(result_size_ptr, result)
    }

    fn check_host_buffer(&mut self) -> Result<(), ApiError> {
        if !self.can_write_to_host_buffer() {
            Err(ApiError::HostBufferFull)
        } else {
            Ok(())
        }
    }

    fn manage_call_contract_host_buffer(
        &mut self,
        result_size_ptr: u32,
        result: CLValue,
    ) -> Result<Result<(), ApiError>, Error> {
        let result_size: u32 = match result.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };

        // leave the host buffer set to `None` if there's nothing to write there
        if result_size != 0 {
            if let Err(error) = self.write_host_buffer(result) {
                return Ok(Err(error));
            }
        }

        let result_size_bytes = result_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self
            .try_get_memory()?
            .set(result_size_ptr, &result_size_bytes)
        {
            return Err(Error::Interpreter(error.into()));
        }

        Ok(Ok(()))
    }

    fn load_named_keys(
        &mut self,
        total_keys_ptr: u32,
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }

        let total_keys: u32 = match self.context.named_keys().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };

        let total_keys_bytes = total_keys.to_le_bytes();
        if let Err(error) = self
            .try_get_memory()?
            .set(total_keys_ptr, &total_keys_bytes)
        {
            return Err(Error::Interpreter(error.into()).into());
        }

        if total_keys == 0 {
            // No need to do anything else, we leave host buffer empty.
            return Ok(Ok(()));
        }

        let named_keys =
            CLValue::from_t(self.context.named_keys().clone()).map_err(Error::CLValue)?;

        let length: u32 = match named_keys.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::BufferTooSmall)),
        };

        if let Err(error) = self.write_host_buffer(named_keys) {
            return Ok(Err(error));
        }

        let length_bytes = length.to_le_bytes();
        if let Err(error) = self.try_get_memory()?.set(result_size_ptr, &length_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    fn create_contract_package(
        &mut self,
        is_locked: PackageStatus,
    ) -> Result<(Package, URef), Error> {
        let access_key = self.context.new_unit_uref()?;
        let contract_package = Package::new(
            access_key,
            EntityVersions::new(),
            BTreeSet::new(),
            Groups::new(),
            is_locked,
        );

        Ok((contract_package, access_key))
    }

    fn create_contract_package_at_hash(
        &mut self,
        lock_status: PackageStatus,
    ) -> Result<([u8; 32], [u8; 32]), Error> {
        let addr = self.context.new_hash_address()?;
        let (contract_package, access_key) = self.create_contract_package(lock_status)?;
        self.context
            .metered_write_gs_unsafe(Key::Package(addr), contract_package)?;
        Ok((addr, access_key.addr()))
    }

    fn create_contract_user_group(
        &mut self,
        contract_package_hash: PackageHash,
        label: String,
        num_new_urefs: u32,
        mut existing_urefs: BTreeSet<URef>,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        let mut contract_package: Package =
            self.context.get_validated_package(contract_package_hash)?;

        let groups = contract_package.groups_mut();
        let new_group = Group::new(label);

        // Ensure group does not already exist
        if groups.contains(&new_group) {
            return Ok(Err(addressable_entity::Error::GroupAlreadyExists.into()));
        }

        // Ensure there are not too many groups
        if groups.len() >= (addressable_entity::MAX_GROUPS as usize) {
            return Ok(Err(addressable_entity::Error::MaxGroupsExceeded.into()));
        }

        // Ensure there are not too many urefs
        let total_urefs: usize =
            groups.total_urefs() + (num_new_urefs as usize) + existing_urefs.len();
        if total_urefs > addressable_entity::MAX_TOTAL_UREFS {
            let err = addressable_entity::Error::MaxTotalURefsExceeded;
            return Ok(Err(ApiError::ContractHeader(err as u8)));
        }

        // Proceed with creating user group
        let mut new_urefs = Vec::with_capacity(num_new_urefs as usize);
        for _ in 0..num_new_urefs {
            let u = self.context.new_unit_uref()?;
            new_urefs.push(u);
        }

        for u in new_urefs.iter().cloned() {
            existing_urefs.insert(u);
        }
        groups.insert(new_group, existing_urefs);

        // check we can write to the host buffer
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        // create CLValue for return value
        let new_urefs_value = CLValue::from_t(new_urefs)?;
        let value_size = new_urefs_value.inner_bytes().len();
        // write return value to buffer
        if let Err(err) = self.write_host_buffer(new_urefs_value) {
            return Ok(Err(err));
        }
        // Write return value size to output location
        let output_size_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self
            .try_get_memory()?
            .set(output_size_ptr, &output_size_bytes)
        {
            return Err(Error::Interpreter(error.into()));
        }

        // Write updated package to the global state
        self.context
            .metered_write_gs_unsafe(contract_package_hash, contract_package)?;

        Ok(Ok(()))
    }

    fn add_session_version(
        &mut self,
        entry_points: EntryPoints,
    ) -> Result<Result<(), ApiError>, Error> {
        if !self.context.entity().is_account_kind() {
            return Err(Error::InvalidContext);
        }

        let package_hash = self.context.entity().package_hash();

        let package = self.context.get_package(package_hash)?;

        let byte_code_hash = self.context.new_hash_address()?;

        let byte_code = {
            let module_bytes = self.get_module_from_entry_points(&entry_points)?;
            ByteCode::new(ByteCodeKind::V1CasperWasm, module_bytes)
        };

        let entity_hash =
            if let Some(entity_hash) = self.context.get_entity_key().into_entity_hash() {
                entity_hash
            } else {
                return Err(Error::UnexpectedKeyVariant(self.context.get_entity_key()));
            };

        let entity = self.context.entity().clone();

        let updated_session_entity =
            entity.update_session_entity(ByteCodeHash::new(byte_code_hash), entry_points);

        self.context.metered_write_gs_unsafe(
            Key::ByteCode(ByteCodeAddr::new_wasm_addr(byte_code_hash)),
            byte_code,
        )?;

        let entity_kind = updated_session_entity.entity_kind();

        if entity_kind != EntityKind::SmartContract {
            return Err(Error::InvalidContext);
        }

        let entity_addr = EntityAddr::new_contract_entity_addr(entity_hash.value());

        self.context
            .metered_write_gs_unsafe(Key::AddressableEntity(entity_addr), updated_session_entity)?;

        self.context
            .metered_write_gs_unsafe(package_hash, package)?;

        Ok(Ok(()))
    }

    #[allow(clippy::too_many_arguments)]
    fn add_contract_version(
        &mut self,
        package_hash: PackageHash,
        version_ptr: u32,
        entry_points: EntryPoints,
        mut named_keys: NamedKeys,
        output_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        if entry_points.contains_stored_session() {
            return Err(Error::InvalidEntryPointType);
        }

        let mut package = self.context.get_package(package_hash)?;

        // Return an error if the contract is locked and has some version associated with it.
        if package.is_locked() {
            return Err(Error::LockedEntity(package_hash));
        }

        let (main_purse, previous_named_keys, action_thresholds, associated_keys, message_topics) =
            self.new_version_entity_parts(&package)?;

        // TODO: EE-1032 - Implement different ways of carrying on existing named keys.
        named_keys.append(previous_named_keys);

        let byte_code_hash = self.context.new_hash_address()?;

        let entity_hash = self.context.new_hash_address()?;

        let protocol_version = self.context.protocol_version();

        let insert_contract_result =
            package.insert_entity_version(protocol_version.value().major, entity_hash.into());

        let byte_code = {
            let module_bytes = self.get_module_from_entry_points(&entry_points)?;
            ByteCode::new(ByteCodeKind::V1CasperWasm, module_bytes)
        };

        self.context.metered_write_gs_unsafe(
            Key::ByteCode(ByteCodeAddr::new_wasm_addr(byte_code_hash)),
            byte_code,
        )?;

        let entity_addr = EntityAddr::new_contract_entity_addr(entity_hash);

        let entity_key = Key::AddressableEntity(entity_addr);

        // Is this still valid??
        // TODO: EE-1032 - Implement different ways of carrying on existing named keys.
        self.context.write_named_keys(entity_addr, named_keys)?;

        let entity = AddressableEntity::new(
            package_hash,
            byte_code_hash.into(),
            entry_points,
            protocol_version,
            main_purse,
            associated_keys,
            action_thresholds,
            message_topics.clone(),
            EntityKind::SmartContract,
        );

        self.context.metered_write_gs_unsafe(entity_key, entity)?;
        self.context
            .metered_write_gs_unsafe(package_hash, package)?;

        for (_, topic_hash) in message_topics.iter() {
            let topic_key = Key::message_topic(entity_hash.into(), *topic_hash);
            let summary = StoredValue::MessageTopic(MessageTopicSummary::new(
                0,
                self.context.get_blocktime(),
            ));
            self.context.metered_write_gs_unsafe(topic_key, summary)?;
        }

        // set return values to buffer
        {
            let hash_bytes = match entity_hash.to_bytes() {
                Ok(bytes) => bytes,
                Err(error) => return Ok(Err(error.into())),
            };

            // Set serialized hash bytes into the output buffer
            if let Err(error) = self.try_get_memory()?.set(output_ptr, &hash_bytes) {
                return Err(Error::Interpreter(error.into()));
            }

            // Set version into VM shared memory
            let version_value: u32 = insert_contract_result.entity_version();
            let version_bytes = version_value.to_le_bytes();
            if let Err(error) = self.try_get_memory()?.set(version_ptr, &version_bytes) {
                return Err(Error::Interpreter(error.into()));
            }
        }

        Ok(Ok(()))
    }

    fn new_version_entity_parts(
        &mut self,
        package: &Package,
    ) -> Result<
        (
            URef,
            NamedKeys,
            ActionThresholds,
            AssociatedKeys,
            MessageTopics,
        ),
        Error,
    > {
        if let Some(previous_entity_hash) = package.current_entity_hash() {
            let previous_entity_key = Key::contract_entity_key(previous_entity_hash);
            let (mut previous_entity, requires_purse_creation) =
                self.context.get_contract_entity(previous_entity_key)?;

            let action_thresholds = previous_entity.action_thresholds().clone();

            let associated_keys = previous_entity.associated_keys().clone();

            if !previous_entity.can_upgrade_with(self.context.authorization_keys()) {
                // Check if the calling entity must be grandfathered into the new
                // addressable entity format
                let account_hash = self.context.get_caller();
                let has_access = self.context.validate_uref(&package.access_key()).is_ok();

                if has_access && !associated_keys.contains_key(&account_hash) {
                    previous_entity.add_associated_key(
                        account_hash,
                        *action_thresholds.upgrade_management(),
                    )?;
                } else {
                    return Err(Error::UpgradeAuthorizationFailure);
                }
            }

            let main_purse = if !requires_purse_creation {
                self.create_purse()?
            } else {
                previous_entity.main_purse()
            };

            let associated_keys = previous_entity.associated_keys().clone();

            let previous_message_topics = previous_entity.message_topics().clone();

            let previous_named_keys = self.context.get_named_keys(previous_entity_key)?;

            return Ok((
                main_purse,
                previous_named_keys,
                action_thresholds,
                associated_keys,
                previous_message_topics,
            ));
        }

        Ok((
            self.create_purse()?,
            NamedKeys::new(),
            ActionThresholds::default(),
            AssociatedKeys::new(self.context.get_caller(), Weight::new(1)),
            MessageTopics::default(),
        ))
    }

    fn disable_contract_version(
        &mut self,
        contract_package_hash: PackageHash,
        contract_hash: AddressableEntityHash,
    ) -> Result<Result<(), ApiError>, Error> {
        let contract_package_key = contract_package_hash.into();
        self.context.validate_key(&contract_package_key)?;

        let mut contract_package: Package =
            self.context.get_validated_package(contract_package_hash)?;

        if contract_package.is_locked() {
            return Err(Error::LockedEntity(contract_package_hash));
        }

        if let Err(err) = contract_package.disable_entity_version(contract_hash) {
            return Ok(Err(err.into()));
        }

        self.context
            .metered_write_gs_unsafe(contract_package_key, contract_package)?;

        Ok(Ok(()))
    }

    fn enable_contract_version(
        &mut self,
        contract_package_hash: PackageHash,
        contract_hash: AddressableEntityHash,
    ) -> Result<Result<(), ApiError>, Error> {
        let contract_package_key = contract_package_hash.into();
        self.context.validate_key(&contract_package_key)?;

        let mut contract_package: Package =
            self.context.get_validated_package(contract_package_hash)?;

        if contract_package.is_locked() {
            return Err(Error::LockedEntity(contract_package_hash));
        }

        if let Err(err) = contract_package.enable_version(contract_hash) {
            return Ok(Err(err.into()));
        }

        self.context
            .metered_write_gs_unsafe(contract_package_key, contract_package)?;

        Ok(Ok(()))
    }

    /// Writes function address (`hash_bytes`) into the Wasm memory (at
    /// `dest_ptr` pointer).
    fn function_address(&mut self, hash_bytes: [u8; 32], dest_ptr: u32) -> Result<(), Trap> {
        self.try_get_memory()?
            .set(dest_ptr, &hash_bytes)
            .map_err(|e| Error::Interpreter(e.into()).into())
    }

    /// Generates new unforgable reference and adds it to the context's
    /// access_rights set.
    fn new_uref(&mut self, uref_ptr: u32, value_ptr: u32, value_size: u32) -> Result<(), Trap> {
        let cl_value = self.cl_value_from_mem(value_ptr, value_size)?; // read initial value from memory
        let uref = self.context.new_uref(StoredValue::CLValue(cl_value))?;
        self.try_get_memory()?
            .set(uref_ptr, &uref.into_bytes().map_err(Error::BytesRepr)?)
            .map_err(|e| Error::Interpreter(e.into()).into())
    }

    /// Writes `value` under `key` in GlobalState.
    fn write(
        &mut self,
        key_ptr: u32,
        key_size: u32,
        value_ptr: u32,
        value_size: u32,
    ) -> Result<(), Trap> {
        let key = self.key_from_mem(key_ptr, key_size)?;
        let cl_value = self.cl_value_from_mem(value_ptr, value_size)?;
        self.context
            .metered_write_gs(key, cl_value)
            .map_err(Into::into)
    }

    /// Records a transfer.
    fn record_transfer(
        &mut self,
        maybe_to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error> {
        if self.context.get_entity_key() != self.context.get_system_entity_key(MINT)? {
            return Err(Error::InvalidContext);
        }

        if self.context.phase() != Phase::Session {
            return Ok(());
        }

        let transfer_addr = self.context.new_transfer_addr()?;
        let transfer = {
            let deploy_hash: DeployHash = self.context.get_deploy_hash();
            let from: AccountHash = self.context.get_caller();
            let fee: U512 = U512::zero(); // TODO
            Transfer::new(deploy_hash, from, maybe_to, source, target, amount, fee, id)
        };
        {
            let transfers = self.context.transfers_mut();
            transfers.push(transfer_addr);
        }
        self.context
            .write_transfer(Key::Transfer(transfer_addr), transfer);
        Ok(())
    }

    /// Records given auction info at a given era id
    fn record_era_info(&mut self, era_info: EraInfo) -> Result<(), Error> {
        if self.context.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidContext);
        }

        if self.context.get_entity_key() != self.context.get_system_entity_key(AUCTION)? {
            return Err(Error::InvalidContext);
        }

        if self.context.phase() != Phase::Session {
            return Ok(());
        }

        self.context.write_era_info(Key::EraSummary, era_info);

        Ok(())
    }

    /// Adds `value` to the cell that `key` points at.
    fn add(
        &mut self,
        key_ptr: u32,
        key_size: u32,
        value_ptr: u32,
        value_size: u32,
    ) -> Result<(), Trap> {
        let key = self.key_from_mem(key_ptr, key_size)?;
        let cl_value = self.cl_value_from_mem(value_ptr, value_size)?;
        self.context
            .metered_add_gs(key, cl_value)
            .map_err(Into::into)
    }

    /// Reads value from the GS living under key specified by `key_ptr` and
    /// `key_size`. Wasm and host communicate through memory that Wasm
    /// module exports. If contract wants to pass data to the host, it has
    /// to tell it [the host] where this data lives in the exported memory
    /// (pass its pointer and length).
    fn read(
        &mut self,
        key_ptr: u32,
        key_size: u32,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }

        let key = self.key_from_mem(key_ptr, key_size)?;
        let cl_value = match self.context.read_gs(&key)? {
            Some(stored_value) => CLValue::try_from(stored_value).map_err(Error::TypeMismatch)?,
            None => return Ok(Err(ApiError::ValueNotFound)),
        };

        let value_size: u32 = match cl_value.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::BufferTooSmall)),
        };

        if let Err(error) = self.write_host_buffer(cl_value) {
            return Ok(Err(error));
        }

        let value_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self.try_get_memory()?.set(output_size_ptr, &value_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Reverts contract execution with a status specified.
    fn revert(&mut self, status: u32) -> Trap {
        Error::Revert(status.into()).into()
    }

    /// Checks if a caller can manage its own associated keys and thresholds.
    ///
    /// On some private chains with administrator keys configured this requires that the caller is
    /// an admin to be able to manage its own keys. If the caller is not an administrator then the
    /// deploy has to be signed by an administrator.
    fn can_manage_keys(&self) -> bool {
        if self
            .context
            .engine_config()
            .administrative_accounts()
            .is_empty()
        {
            // Public chain
            return self
                .context
                .entity()
                .can_manage_keys_with(self.context.authorization_keys());
        }

        if self
            .context
            .engine_config()
            .is_administrator(&self.context.get_caller())
        {
            return true;
        }

        // If caller is not an admin, check if deploy was co-signed by admin account.
        self.context.is_authorized_by_admin()
    }

    fn add_associated_key(
        &mut self,
        account_hash_ptr: u32,
        account_hash_size: usize,
        weight_value: u8,
    ) -> Result<i32, Trap> {
        let account_hash = {
            // Account hash as serialized bytes
            let source_serialized = self.bytes_from_mem(account_hash_ptr, account_hash_size)?;
            // Account hash deserialized
            let source: AccountHash =
                bytesrepr::deserialize_from_slice(source_serialized).map_err(Error::BytesRepr)?;
            source
        };
        let weight = Weight::new(weight_value);

        if !self.can_manage_keys() {
            return Ok(AddKeyFailure::PermissionDenied as i32);
        }

        match self.context.add_associated_key(account_hash, weight) {
            Ok(_) => Ok(0),
            // This relies on the fact that `AddKeyFailure` is represented as
            // i32 and first variant start with number `1`, so all other variants
            // are greater than the first one, so it's safe to assume `0` is success,
            // and any error is greater than 0.
            Err(Error::AddKeyFailure(e)) => Ok(e as i32),
            // Any other variant just pass as `Trap`
            Err(e) => Err(e.into()),
        }
    }

    fn remove_associated_key(
        &mut self,
        account_hash_ptr: u32,
        account_hash_size: usize,
    ) -> Result<i32, Trap> {
        let account_hash = {
            // Account hash as serialized bytes
            let source_serialized = self.bytes_from_mem(account_hash_ptr, account_hash_size)?;
            // Account hash deserialized
            let source: AccountHash =
                bytesrepr::deserialize_from_slice(source_serialized).map_err(Error::BytesRepr)?;
            source
        };

        if !self.can_manage_keys() {
            return Ok(RemoveKeyFailure::PermissionDenied as i32);
        }

        match self.context.remove_associated_key(account_hash) {
            Ok(_) => Ok(0),
            Err(Error::RemoveKeyFailure(e)) => Ok(e as i32),
            Err(e) => Err(e.into()),
        }
    }

    fn update_associated_key(
        &mut self,
        account_hash_ptr: u32,
        account_hash_size: usize,
        weight_value: u8,
    ) -> Result<i32, Trap> {
        let account_hash = {
            // Account hash as serialized bytes
            let source_serialized = self.bytes_from_mem(account_hash_ptr, account_hash_size)?;
            // Account hash deserialized
            let source: AccountHash =
                bytesrepr::deserialize_from_slice(source_serialized).map_err(Error::BytesRepr)?;
            source
        };
        let weight = Weight::new(weight_value);

        if !self.can_manage_keys() {
            return Ok(UpdateKeyFailure::PermissionDenied as i32);
        }

        match self.context.update_associated_key(account_hash, weight) {
            Ok(_) => Ok(0),
            // This relies on the fact that `UpdateKeyFailure` is represented as
            // i32 and first variant start with number `1`, so all other variants
            // are greater than the first one, so it's safe to assume `0` is success,
            // and any error is greater than 0.
            Err(Error::UpdateKeyFailure(e)) => Ok(e as i32),
            // Any other variant just pass as `Trap`
            Err(e) => Err(e.into()),
        }
    }

    fn set_action_threshold(
        &mut self,
        action_type_value: u32,
        threshold_value: u8,
    ) -> Result<i32, Trap> {
        if !self.can_manage_keys() {
            return Ok(SetThresholdFailure::PermissionDeniedError as i32);
        }

        match ActionType::try_from(action_type_value) {
            Ok(action_type) => {
                let threshold = Weight::new(threshold_value);
                match self.context.set_action_threshold(action_type, threshold) {
                    Ok(_) => Ok(0),
                    Err(Error::SetThresholdFailure(e)) => Ok(e as i32),
                    Err(error) => Err(error.into()),
                }
            }
            Err(_) => Err(Trap::Code(TrapCode::Unreachable)),
        }
    }

    /// Looks up the public mint contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_mint_contract(&self) -> Result<AddressableEntityHash, Error> {
        self.context.get_system_contract(MINT)
    }

    /// Looks up the public handle payment contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_handle_payment_contract(&self) -> Result<AddressableEntityHash, Error> {
        self.context.get_system_contract(HANDLE_PAYMENT)
    }

    /// Looks up the public standard payment contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_standard_payment_contract(&self) -> Result<AddressableEntityHash, Error> {
        self.context.get_system_contract(STANDARD_PAYMENT)
    }

    /// Looks up the public auction contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_auction_contract(&self) -> Result<AddressableEntityHash, Error> {
        self.context.get_system_contract(AUCTION)
    }

    /// Calls the `read_base_round_reward` method on the mint contract at the given mint
    /// contract key
    fn mint_read_base_round_reward(
        &mut self,
        mint_contract_hash: AddressableEntityHash,
    ) -> Result<U512, Error> {
        let gas_counter = self.gas_counter();
        let call_result = self.call_contract(
            mint_contract_hash,
            mint::METHOD_READ_BASE_ROUND_REWARD,
            RuntimeArgs::default(),
        );
        self.set_gas_counter(gas_counter);

        let reward = call_result?.into_t()?;
        Ok(reward)
    }

    /// Calls the `mint` method on the mint contract at the given mint
    /// contract key
    fn mint_mint(
        &mut self,
        mint_contract_hash: AddressableEntityHash,
        amount: U512,
    ) -> Result<URef, Error> {
        let gas_counter = self.gas_counter();
        let runtime_args = {
            let mut runtime_args = RuntimeArgs::new();
            runtime_args.insert(mint::ARG_AMOUNT, amount)?;
            runtime_args
        };
        let call_result = self.call_contract(mint_contract_hash, mint::METHOD_MINT, runtime_args);
        self.set_gas_counter(gas_counter);

        let result: Result<URef, mint::Error> = call_result?.into_t()?;
        Ok(result.map_err(system::Error::from)?)
    }

    /// Calls the `reduce_total_supply` method on the mint contract at the given mint
    /// contract key
    fn mint_reduce_total_supply(
        &mut self,
        mint_contract_hash: AddressableEntityHash,
        amount: U512,
    ) -> Result<(), Error> {
        let gas_counter = self.gas_counter();
        let runtime_args = {
            let mut runtime_args = RuntimeArgs::new();
            runtime_args.insert(mint::ARG_AMOUNT, amount)?;
            runtime_args
        };
        let call_result = self.call_contract(
            mint_contract_hash,
            mint::METHOD_REDUCE_TOTAL_SUPPLY,
            runtime_args,
        );
        self.set_gas_counter(gas_counter);

        let result: Result<(), mint::Error> = call_result?.into_t()?;
        Ok(result.map_err(system::Error::from)?)
    }

    /// Calls the "create" method on the mint contract at the given mint
    /// contract key
    fn mint_create(&mut self, mint_contract_hash: AddressableEntityHash) -> Result<URef, Error> {
        let result =
            self.call_contract(mint_contract_hash, mint::METHOD_CREATE, RuntimeArgs::new());
        let purse = result?.into_t()?;
        Ok(purse)
    }

    fn create_purse(&mut self) -> Result<URef, Error> {
        let _scoped_host_function_flag = self.host_function_flag.enter_host_function_scope();
        self.mint_create(self.get_mint_contract()?)
    }

    /// Calls the "transfer" method on the mint contract at the given mint
    /// contract key
    fn mint_transfer(
        &mut self,
        mint_contract_hash: AddressableEntityHash,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<Result<(), mint::Error>, Error> {
        self.context.validate_uref(&source)?;

        let args_values = {
            let mut runtime_args = RuntimeArgs::new();
            runtime_args.insert(mint::ARG_TO, to)?;
            runtime_args.insert(mint::ARG_SOURCE, source)?;
            runtime_args.insert(mint::ARG_TARGET, target)?;
            runtime_args.insert(mint::ARG_AMOUNT, amount)?;
            runtime_args.insert(mint::ARG_ID, id)?;
            runtime_args
        };

        let gas_counter = self.gas_counter();
        let call_result =
            self.call_contract(mint_contract_hash, mint::METHOD_TRANSFER, args_values);
        self.set_gas_counter(gas_counter);

        Ok(call_result?.into_t()?)
    }

    /// Creates a new account at a given public key, transferring a given amount
    /// of motes from the given source purse to the new account's purse.
    fn transfer_to_new_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
        id: Option<u64>,
    ) -> Result<TransferResult, Error> {
        let mint_contract_hash = self.get_mint_contract()?;

        if !self.context.engine_config().allow_unrestricted_transfers()
            && self.context.get_caller() != PublicKey::System.to_account_hash()
            && !self
                .context
                .engine_config()
                .is_administrator(&self.context.get_caller())
            && !self.context.engine_config().is_administrator(&target)
        {
            return Err(Error::DisabledUnrestrictedTransfers);
        }

        // A precondition check that verifies that the transfer can be done
        // as the source purse has enough funds to cover the transfer.
        if amount > self.get_balance(source)?.unwrap_or_default() {
            return Ok(Err(mint::Error::InsufficientFunds.into()));
        }

        let target_purse = self.mint_create(mint_contract_hash)?;

        if source == target_purse {
            return Ok(Err(mint::Error::EqualSourceAndTarget.into()));
        }

        let result = self.mint_transfer(
            mint_contract_hash,
            Some(target),
            source,
            target_purse.with_access_rights(AccessRights::ADD),
            amount,
            id,
        );

        // We granted a temporary access rights bit to newly created main purse as part of
        // `mint_create` call, and we need to remove it to avoid leakage of access rights.

        self.context
            .remove_access(target_purse.addr(), target_purse.access_rights());

        match result? {
            Ok(()) => {
                let protocol_version = self.context.protocol_version();
                let byte_code_hash = ByteCodeHash::default();
                let entity_hash = AddressableEntityHash::new(self.context.new_hash_address()?);
                let package_hash = PackageHash::new(self.context.new_hash_address()?);
                let main_purse = target_purse;
                let associated_keys = AssociatedKeys::new(target, Weight::new(1));
                let entry_points = EntryPoints::new();
                let message_topics = MessageTopics::default();

                let entity = AddressableEntity::new(
                    package_hash,
                    byte_code_hash,
                    entry_points,
                    protocol_version,
                    main_purse,
                    associated_keys,
                    ActionThresholds::default(),
                    message_topics,
                    EntityKind::Account(target),
                );

                let access_key = self.context.new_unit_uref()?;
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

                self.context
                    .metered_write_gs_unsafe(entity_key, StoredValue::AddressableEntity(entity))?;

                let contract_package_key: Key = package_hash.into();

                self.context
                    .metered_write_gs_unsafe(contract_package_key, StoredValue::Package(package))?;

                let contract_by_account = CLValue::from_t(entity_key)?;

                let target_key = Key::Account(target);

                self.context.metered_write_gs_unsafe(
                    target_key,
                    StoredValue::CLValue(contract_by_account),
                )?;

                Ok(Ok(TransferredTo::NewAccount))
            }
            Err(mint_error) => Ok(Err(mint_error.into())),
        }
    }

    /// Transferring a given amount of motes from the given source purse to the
    /// new account's purse. Requires that the [`URef`]s have already
    /// been created by the mint contract (or are the genesis account's).
    fn transfer_to_existing_account(
        &mut self,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<TransferResult, Error> {
        let mint_contract_key = self.get_mint_contract()?;

        match self.mint_transfer(mint_contract_key, to, source, target, amount, id)? {
            Ok(()) => Ok(Ok(TransferredTo::ExistingAccount)),
            Err(error) => Ok(Err(error.into())),
        }
    }

    /// Transfers `amount` of motes from default purse of the account to
    /// `target` account. If that account does not exist, creates one.
    fn transfer_to_account(
        &mut self,
        target: AccountHash,
        amount: U512,
        id: Option<u64>,
    ) -> Result<TransferResult, Error> {
        let source = self.context.get_main_purse()?;
        self.transfer_from_purse_to_account_hash(source, target, amount, id)
    }

    /// Transfers `amount` of motes from `source` purse to `target` account.
    /// If that account does not exist, creates one.
    fn transfer_from_purse_to_account_hash(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
        id: Option<u64>,
    ) -> Result<TransferResult, Error> {
        let _scoped_host_function_flag = self.host_function_flag.enter_host_function_scope();
        let target_key = Key::Account(target);

        // Look up the account at the given public key's address
        match self.context.read_gs(&target_key)? {
            None => {
                // If no account exists, create a new account and transfer the amount to its
                // purse.

                self.transfer_to_new_account(source, target, amount, id)
            }
            Some(StoredValue::CLValue(account)) => {
                // Attenuate the target main purse
                let entity_key = CLValue::into_t::<Key>(account)?;
                let target_uref = if let Some(StoredValue::AddressableEntity(entity)) =
                    self.context.read_gs(&entity_key)?
                {
                    entity.main_purse_add_only()
                } else {
                    let contract_hash = if let Some(entity_hash) = entity_key
                        .into_entity_hash_addr()
                        .map(AddressableEntityHash::new)
                    {
                        entity_hash
                    } else {
                        return Err(Error::UnexpectedKeyVariant(entity_key));
                    };
                    return Err(Error::InvalidEntity(contract_hash));
                };

                if source.with_access_rights(AccessRights::ADD) == target_uref {
                    return Ok(Ok(TransferredTo::ExistingAccount));
                }

                // Upsert ADD access to caller on target allowing deposit of motes; this will be
                // revoked after the transfer is completed if caller did not already have ADD access
                let granted_access = self.context.grant_access(target_uref);

                // If an account exists, transfer the amount to its purse
                let transfer_result = self.transfer_to_existing_account(
                    Some(target),
                    source,
                    target_uref,
                    amount,
                    id,
                );

                // Remove from caller temporarily granted ADD access on target.
                if let GrantedAccess::Granted {
                    uref_addr,
                    newly_granted_access_rights,
                } = granted_access
                {
                    self.context
                        .remove_access(uref_addr, newly_granted_access_rights)
                }
                transfer_result
            }
            Some(StoredValue::Account(account)) => {
                self.transfer_from_purse_to_account(source, &account, amount, id)
            }
            Some(_) => {
                // If some other value exists, return an error
                Err(Error::AccountNotFound(target_key))
            }
        }
    }

    fn transfer_from_purse_to_account(
        &mut self,
        source: URef,
        target_account: &Account,
        amount: U512,
        id: Option<u64>,
    ) -> Result<TransferResult, Error> {
        // Attenuate the target main purse
        let target_uref = target_account.main_purse_add_only();

        if source.with_access_rights(AccessRights::ADD) == target_uref {
            return Ok(Ok(TransferredTo::ExistingAccount));
        }

        // Grant ADD access to caller on target allowing deposit of motes; this will be
        // revoked after the transfer is completed if caller did not already have ADD access
        let granted_access = self.context.grant_access(target_uref);

        // If an account exists, transfer the amount to its purse
        let transfer_result = self.transfer_to_existing_account(
            Some(target_account.account_hash()),
            source,
            target_uref,
            amount,
            id,
        );

        // Remove from caller temporarily granted ADD access on target.
        if let GrantedAccess::Granted {
            uref_addr,
            newly_granted_access_rights,
        } = granted_access
        {
            self.context
                .remove_access(uref_addr, newly_granted_access_rights)
        }
        transfer_result
    }

    /// Transfers `amount` of motes from `source` purse to `target` purse.
    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<Result<(), mint::Error>, Error> {
        self.context.validate_uref(&source)?;
        let mint_contract_key = self.get_mint_contract()?;
        match self.mint_transfer(mint_contract_key, None, source, target, amount, id)? {
            Ok(()) => Ok(Ok(())),
            Err(mint_error) => Ok(Err(mint_error)),
        }
    }

    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        let maybe_value = self.context.read_gs_direct(&Key::Balance(purse.addr()))?;
        match maybe_value {
            Some(StoredValue::CLValue(value)) => {
                let value = CLValue::into_t(value)?;
                Ok(Some(value))
            }
            Some(_) => Err(Error::UnexpectedStoredValueVariant),
            None => Ok(None),
        }
    }

    fn get_balance_host_buffer(
        &mut self,
        purse_ptr: u32,
        purse_size: usize,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }

        let purse: URef = {
            let bytes = self.bytes_from_mem(purse_ptr, purse_size)?;
            match bytesrepr::deserialize_from_slice(bytes) {
                Ok(purse) => purse,
                Err(error) => return Ok(Err(error.into())),
            }
        };

        let balance = match self.get_balance(purse)? {
            Some(balance) => balance,
            None => return Ok(Err(ApiError::InvalidPurse)),
        };

        let balance_cl_value = match CLValue::from_t(balance) {
            Ok(cl_value) => cl_value,
            Err(error) => return Ok(Err(error.into())),
        };

        let balance_size = balance_cl_value.inner_bytes().len() as i32;
        if let Err(error) = self.write_host_buffer(balance_cl_value) {
            return Ok(Err(error));
        }

        let balance_size_bytes = balance_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self
            .try_get_memory()?
            .set(output_size_ptr, &balance_size_bytes)
        {
            return Err(Error::Interpreter(error.into()));
        }

        Ok(Ok(()))
    }

    fn get_system_contract(
        &mut self,
        system_contract_index: u32,
        dest_ptr: u32,
        _dest_size: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        let contract_hash: AddressableEntityHash =
            match SystemEntityType::try_from(system_contract_index) {
                Ok(SystemEntityType::Mint) => self.get_mint_contract()?,
                Ok(SystemEntityType::HandlePayment) => self.get_handle_payment_contract()?,
                Ok(SystemEntityType::StandardPayment) => self.get_standard_payment_contract()?,
                Ok(SystemEntityType::Auction) => self.get_auction_contract()?,
                Err(error) => return Ok(Err(error)),
            };

        match self.try_get_memory()?.set(dest_ptr, contract_hash.as_ref()) {
            Ok(_) => Ok(Ok(())),
            Err(error) => Err(Error::Interpreter(error.into()).into()),
        }
    }

    /// If host_buffer set, clears the host_buffer and returns value, else None
    pub fn take_host_buffer(&mut self) -> Option<CLValue> {
        self.host_buffer.take()
    }

    /// Checks if a write to host buffer can happen.
    ///
    /// This will check if the host buffer is empty.
    fn can_write_to_host_buffer(&self) -> bool {
        self.host_buffer.is_none()
    }

    /// Overwrites data in host buffer only if it's in empty state
    fn write_host_buffer(&mut self, data: CLValue) -> Result<(), ApiError> {
        match self.host_buffer {
            Some(_) => return Err(ApiError::HostBufferFull),
            None => self.host_buffer = Some(data),
        }
        Ok(())
    }

    fn read_host_buffer(
        &mut self,
        dest_ptr: u32,
        dest_size: usize,
        bytes_written_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        let (_cl_type, serialized_value) = match self.take_host_buffer() {
            None => return Ok(Err(ApiError::HostBufferEmpty)),
            Some(cl_value) => cl_value.destructure(),
        };

        if serialized_value.len() > u32::max_value() as usize {
            return Ok(Err(ApiError::OutOfMemory));
        }
        if serialized_value.len() > dest_size {
            return Ok(Err(ApiError::BufferTooSmall));
        }

        // Slice data, so if `dest_size` is larger than host_buffer size, it will take host_buffer
        // as whole.
        let sliced_buf = &serialized_value[..cmp::min(dest_size, serialized_value.len())];
        if let Err(error) = self.try_get_memory()?.set(dest_ptr, sliced_buf) {
            return Err(Error::Interpreter(error.into()));
        }

        // Never panics because we check that `serialized_value.len()` fits in `u32`.
        let bytes_written: u32 = sliced_buf
            .len()
            .try_into()
            .expect("Size of buffer should fit within limit");
        let bytes_written_data = bytes_written.to_le_bytes();

        if let Err(error) = self
            .try_get_memory()?
            .set(bytes_written_ptr, &bytes_written_data)
        {
            return Err(Error::Interpreter(error.into()));
        }

        Ok(Ok(()))
    }

    #[cfg(feature = "test-support")]
    fn print(&mut self, text_ptr: u32, text_size: u32) -> Result<(), Trap> {
        let text = self.string_from_mem(text_ptr, text_size)?;
        println!("{}", text);
        Ok(())
    }

    fn get_named_arg_size(
        &mut self,
        name_ptr: u32,
        name_size: usize,
        size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        let name_bytes = self.bytes_from_mem(name_ptr, name_size)?;
        let name = String::from_utf8_lossy(&name_bytes);

        let arg_size: u32 = match self.context.args().get(&name) {
            Some(arg) if arg.inner_bytes().len() > u32::max_value() as usize => {
                return Ok(Err(ApiError::OutOfMemory));
            }
            Some(arg) => {
                // SAFETY: Safe to unwrap as we asserted length above
                arg.inner_bytes()
                    .len()
                    .try_into()
                    .expect("Should fit within the range")
            }
            None => return Ok(Err(ApiError::MissingArgument)),
        };

        let arg_size_bytes = arg_size.to_le_bytes(); // Wasm is little-endian

        if let Err(e) = self.try_get_memory()?.set(size_ptr, &arg_size_bytes) {
            return Err(Error::Interpreter(e.into()).into());
        }

        Ok(Ok(()))
    }

    fn get_named_arg(
        &mut self,
        name_ptr: u32,
        name_size: usize,
        output_ptr: u32,
        output_size: usize,
    ) -> Result<Result<(), ApiError>, Trap> {
        let name_bytes = self.bytes_from_mem(name_ptr, name_size)?;
        let name = String::from_utf8_lossy(&name_bytes);

        let arg = match self.context.args().get(&name) {
            Some(arg) => arg,
            None => return Ok(Err(ApiError::MissingArgument)),
        };

        if arg.inner_bytes().len() > output_size {
            return Ok(Err(ApiError::OutOfMemory));
        }

        if let Err(error) = self
            .try_get_memory()?
            .set(output_ptr, &arg.inner_bytes()[..output_size])
        {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Enforce group access restrictions (if any) on attempts to call an `EntryPoint`.
    fn validate_entry_point_access(
        &self,
        package: &Package,
        name: &str,
        access: &EntryPointAccess,
    ) -> Result<(), Error> {
        match access {
            EntryPointAccess::Public => Ok(()),
            EntryPointAccess::Groups(group_names) => {
                if group_names.is_empty() {
                    // Exits early in a special case of empty list of groups regardless of the group
                    // checking logic below it.
                    return Err(Error::InvalidContext);
                }

                let find_result = group_names.iter().find(|&group_name| {
                    package
                        .groups()
                        .get(group_name)
                        .and_then(|urefs| {
                            urefs
                                .iter()
                                .find(|&uref| self.context.validate_uref(uref).is_ok())
                        })
                        .is_some()
                });

                if find_result.is_none() {
                    return Err(Error::InvalidContext);
                }

                Ok(())
            }
            EntryPointAccess::Template => Err(Error::TemplateMethod(name.to_string())),
        }
    }

    /// Remove a user group from access to a contract
    fn remove_contract_user_group(
        &mut self,
        package_key: PackageHash,
        label: Group,
    ) -> Result<Result<(), ApiError>, Error> {
        let mut package: Package = self.context.get_validated_package(package_key)?;

        let group_to_remove = Group::new(label);
        let groups = package.groups_mut();

        // Ensure group exists in groups
        if !groups.contains(&group_to_remove) {
            return Ok(Err(addressable_entity::Error::GroupDoesNotExist.into()));
        }

        // Remove group if it is not referenced by at least one entry_point in active versions.
        let versions = package.versions();
        for entity_hash in versions.contract_hashes() {
            let entry_points = {
                let entity: AddressableEntity = self
                    .context
                    .read_gs_typed(&Key::contract_entity_key(*entity_hash))?;
                entity.entry_points().clone().take_entry_points()
            };
            for entry_point in entry_points {
                match entry_point.access() {
                    EntryPointAccess::Public | EntryPointAccess::Template => {
                        continue;
                    }
                    EntryPointAccess::Groups(groups) => {
                        if groups.contains(&group_to_remove) {
                            return Ok(Err(addressable_entity::Error::GroupInUse.into()));
                        }
                    }
                }
            }
        }

        if !package.remove_group(&group_to_remove) {
            return Ok(Err(addressable_entity::Error::GroupInUse.into()));
        }

        // Write updated package to the global state
        self.context.metered_write_gs_unsafe(package_key, package)?;
        Ok(Ok(()))
    }

    #[allow(clippy::too_many_arguments)]
    fn provision_contract_user_group_uref(
        &mut self,
        package_ptr: u32,
        package_size: u32,
        label_ptr: u32,
        label_size: u32,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        let contract_package_hash = self.t_from_mem(package_ptr, package_size)?;
        let label: String = self.t_from_mem(label_ptr, label_size)?;
        let mut contract_package = self.context.get_validated_package(contract_package_hash)?;
        let groups = contract_package.groups_mut();

        let group_label = Group::new(label);

        // Ensure there are not too many urefs
        if groups.total_urefs() + 1 > addressable_entity::MAX_TOTAL_UREFS {
            return Ok(Err(addressable_entity::Error::MaxTotalURefsExceeded.into()));
        }

        // Ensure given group exists and does not exceed limits
        let group = match groups.get_mut(&group_label) {
            Some(group) if group.len() + 1 > addressable_entity::MAX_GROUPS as usize => {
                // Ensures there are not too many groups to fit in amount of new urefs
                return Ok(Err(addressable_entity::Error::MaxTotalURefsExceeded.into()));
            }
            Some(group) => group,
            None => return Ok(Err(addressable_entity::Error::GroupDoesNotExist.into())),
        };

        // Proceed with creating new URefs
        let new_uref = self.context.new_unit_uref()?;
        if !group.insert(new_uref) {
            return Ok(Err(addressable_entity::Error::URefAlreadyExists.into()));
        }

        // check we can write to the host buffer
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        // create CLValue for return value
        let new_uref_value = CLValue::from_t(new_uref)?;
        let value_size = new_uref_value.inner_bytes().len();
        // write return value to buffer
        if let Err(err) = self.write_host_buffer(new_uref_value) {
            return Ok(Err(err));
        }
        // Write return value size to output location
        let output_size_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self
            .try_get_memory()?
            .set(output_size_ptr, &output_size_bytes)
        {
            return Err(Error::Interpreter(error.into()));
        }

        // Write updated package to the global state
        self.context
            .metered_write_gs_unsafe(contract_package_hash, contract_package)?;

        Ok(Ok(()))
    }

    #[allow(clippy::too_many_arguments)]
    fn remove_contract_user_group_urefs(
        &mut self,
        package_ptr: u32,
        package_size: u32,
        label_ptr: u32,
        label_size: u32,
        urefs_ptr: u32,
        urefs_size: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        let contract_package_hash: PackageHash = self.t_from_mem(package_ptr, package_size)?;
        let label: String = self.t_from_mem(label_ptr, label_size)?;
        let urefs: BTreeSet<URef> = self.t_from_mem(urefs_ptr, urefs_size)?;

        let mut contract_package = self.context.get_validated_package(contract_package_hash)?;

        let groups = contract_package.groups_mut();
        let group_label = Group::new(label);

        let group = match groups.get_mut(&group_label) {
            Some(group) => group,
            None => return Ok(Err(addressable_entity::Error::GroupDoesNotExist.into())),
        };

        if urefs.is_empty() {
            return Ok(Ok(()));
        }

        for uref in urefs {
            if !group.remove(&uref) {
                return Ok(Err(addressable_entity::Error::UnableToRemoveURef.into()));
            }
        }
        // Write updated package to the global state
        self.context
            .metered_write_gs_unsafe(contract_package_hash, contract_package)?;

        Ok(Ok(()))
    }

    /// Calculate gas cost for a host function
    fn charge_host_function_call<T>(
        &mut self,
        host_function: &HostFunction<T>,
        weights: T,
    ) -> Result<(), Trap>
    where
        T: AsRef<[HostFunctionCost]> + Copy,
    {
        let cost = host_function.calculate_gas_cost(weights);
        self.gas(cost)?;
        Ok(())
    }

    /// Creates a dictionary
    fn new_dictionary(&mut self, output_size_ptr: u32) -> Result<Result<(), ApiError>, Error> {
        // check we can write to the host buffer
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }

        // Create new URef
        let new_uref = self.context.new_unit_uref()?;

        // create CLValue for return value
        let new_uref_value = CLValue::from_t(new_uref)?;
        let value_size = new_uref_value.inner_bytes().len();
        // write return value to buffer
        if let Err(err) = self.write_host_buffer(new_uref_value) {
            return Ok(Err(err));
        }
        // Write return value size to output location
        let output_size_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self
            .try_get_memory()?
            .set(output_size_ptr, &output_size_bytes)
        {
            return Err(Error::Interpreter(error.into()));
        }

        Ok(Ok(()))
    }

    /// Reads the `value` under a `key` in a dictionary
    fn dictionary_get(
        &mut self,
        uref_ptr: u32,
        uref_size: u32,
        dictionary_item_key_bytes_ptr: u32,
        dictionary_item_key_bytes_size: u32,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        // check we can write to the host buffer
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }

        let uref: URef = self.t_from_mem(uref_ptr, uref_size)?;
        let dictionary_item_key = self.checked_memory_slice(
            dictionary_item_key_bytes_ptr as usize,
            dictionary_item_key_bytes_size as usize,
            |utf8_bytes| std::str::from_utf8(utf8_bytes).map(ToOwned::to_owned),
        )?;

        let dictionary_item_key = if let Ok(item_key) = dictionary_item_key {
            item_key
        } else {
            return Ok(Err(ApiError::InvalidDictionaryItemKey));
        };

        let cl_value = match self.context.dictionary_get(uref, &dictionary_item_key)? {
            Some(cl_value) => cl_value,
            None => return Ok(Err(ApiError::ValueNotFound)),
        };

        let value_size: u32 = match cl_value.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::BufferTooSmall)),
        };

        if let Err(error) = self.write_host_buffer(cl_value) {
            return Ok(Err(error));
        }

        let value_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self.try_get_memory()?.set(output_size_ptr, &value_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Reads the `value` under a `Key::Dictionary`.
    fn dictionary_read(
        &mut self,
        key_ptr: u32,
        key_size: u32,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }

        let dictionary_key = self.key_from_mem(key_ptr, key_size)?;
        let cl_value = match self.context.dictionary_read(dictionary_key)? {
            Some(cl_value) => cl_value,
            None => return Ok(Err(ApiError::ValueNotFound)),
        };

        let value_size: u32 = match cl_value.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::BufferTooSmall)),
        };

        if let Err(error) = self.write_host_buffer(cl_value) {
            return Ok(Err(error));
        }

        let value_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self.try_get_memory()?.set(output_size_ptr, &value_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Writes a `key`, `value` pair in a dictionary
    fn dictionary_put(
        &mut self,
        uref_ptr: u32,
        uref_size: u32,
        key_ptr: u32,
        key_size: u32,
        value_ptr: u32,
        value_size: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        let uref: URef = self.t_from_mem(uref_ptr, uref_size)?;
        let dictionary_item_key_bytes = {
            if (key_size as usize) > DICTIONARY_ITEM_KEY_MAX_LENGTH {
                return Ok(Err(ApiError::DictionaryItemKeyExceedsLength));
            }
            self.checked_memory_slice(key_ptr as usize, key_size as usize, |data| {
                std::str::from_utf8(data).map(ToOwned::to_owned)
            })?
        };

        let dictionary_item_key = if let Ok(item_key) = dictionary_item_key_bytes {
            item_key
        } else {
            return Ok(Err(ApiError::InvalidDictionaryItemKey));
        };
        let cl_value = self.cl_value_from_mem(value_ptr, value_size)?;
        if let Err(e) = self
            .context
            .dictionary_put(uref, &dictionary_item_key, cl_value)
        {
            return Err(Trap::from(e));
        }
        Ok(Ok(()))
    }

    /// Checks if immediate caller is a system contract or account.
    ///
    /// For cases where call stack is only the session code, then this method returns `true` if the
    /// caller is system, or `false` otherwise.
    fn is_system_immediate_caller(&self) -> Result<bool, Error> {
        let immediate_caller = match self.get_immediate_caller() {
            Some(call_stack_element) => call_stack_element,
            None => {
                // Immediate caller is assumed to exist at a time this check is run.
                return Ok(false);
            }
        };

        match immediate_caller {
            CallStackElement::Session { account_hash } => {
                // This case can happen during genesis where we're setting up purses for accounts.
                Ok(account_hash == &PublicKey::System.to_account_hash())
            }
            CallStackElement::AddressableEntity {
                entity_hash: contract_hash,
                ..
            } => Ok(self.context.is_system_addressable_entity(contract_hash)?),
        }
    }

    fn load_authorization_keys(
        &mut self,
        len_ptr: u32,
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }

        // A set of keys is converted into a vector so it can be written to a host buffer
        let authorization_keys =
            Vec::from_iter(self.context.authorization_keys().clone().into_iter());

        let total_keys: u32 = match authorization_keys.len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };
        let total_keys_bytes = total_keys.to_le_bytes();
        if let Err(error) = self.try_get_memory()?.set(len_ptr, &total_keys_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        if total_keys == 0 {
            // No need to do anything else, we leave host buffer empty.
            return Ok(Ok(()));
        }

        let authorization_keys = CLValue::from_t(authorization_keys).map_err(Error::CLValue)?;

        let length: u32 = match authorization_keys.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };
        if let Err(error) = self.write_host_buffer(authorization_keys) {
            return Ok(Err(error));
        }

        let length_bytes = length.to_le_bytes();
        if let Err(error) = self.try_get_memory()?.set(result_size_ptr, &length_bytes) {
            return Err(Error::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    fn prune(&mut self, key: Key) {
        self.context.prune_gs_unsafe(key);
    }

    pub(crate) fn migrate_contract_and_contract_package(
        &mut self,
        contract_hash: AddressableEntityHash,
    ) -> Result<AddressableEntity, Error> {
        let maybe_legacy_contract = self.context.read_gs(&Key::Hash(contract_hash.value()))?;
        match maybe_legacy_contract {
            Some(StoredValue::Contract(contract)) => {
                let contract_package_key = Key::Hash(contract.contract_package_hash().value());

                let legacy_contract_package: ContractPackage =
                    self.context.read_gs_typed(&contract_package_key)?;

                let package: Package = legacy_contract_package.into();

                let access_uref = &package.access_key();

                let package_key = Key::Package(contract.contract_package_hash().value());

                self.context
                    .metered_write_gs_unsafe(package_key, StoredValue::Package(package))?;

                let entity_main_purse = self.create_purse()?;

                let associated_keys = if self.context.validate_uref(access_uref).is_ok() {
                    AssociatedKeys::new(self.context.get_caller(), Weight::new(1))
                } else {
                    AssociatedKeys::default()
                };

                let contract_addr = EntityAddr::new_contract_entity_addr(contract_hash.value());

                self.context
                    .write_named_keys(contract_addr, contract.named_keys().clone())?;

                let updated_entity = AddressableEntity::new(
                    PackageHash::new(contract.contract_package_hash().value()),
                    ByteCodeHash::new(contract.contract_wasm_hash().value()),
                    contract.entry_points().clone(),
                    self.context.protocol_version(),
                    entity_main_purse,
                    associated_keys,
                    ActionThresholds::default(),
                    MessageTopics::default(),
                    EntityKind::SmartContract,
                );

                let previous_wasm = self.context.read_gs_typed::<ContractWasm>(&Key::Hash(
                    contract.contract_wasm_hash().value(),
                ))?;

                let byte_code_key = Key::byte_code_key(ByteCodeAddr::new_wasm_addr(
                    updated_entity.byte_code_addr(),
                ));

                let byte_code: ByteCode = previous_wasm.into();

                self.context
                    .metered_write_gs_unsafe(byte_code_key, StoredValue::ByteCode(byte_code))?;

                let entity_key = Key::contract_entity_key(contract_hash);

                self.context
                    .metered_write_gs_unsafe(entity_key, updated_entity.clone())?;
                Ok(updated_entity)
            }
            Some(_) => Err(Error::UnexpectedStoredValueVariant),
            None => Err(Error::InvalidEntity(contract_hash)),
        }
    }

    fn add_message_topic(&mut self, topic_name: &str) -> Result<Result<(), ApiError>, Error> {
        let topic_hash = crypto::blake2b(topic_name).into();

        self.context
            .add_message_topic(topic_name, topic_hash)
            .map(|ret| ret.map_err(ApiError::from))
    }

    fn emit_message(
        &mut self,
        topic_name: &str,
        message: MessagePayload,
    ) -> Result<Result<(), ApiError>, Trap> {
        let entity_addr = self
            .context
            .get_entity_key()
            .into_entity_hash()
            .ok_or(Error::InvalidContext)?;

        let topic_name_hash = crypto::blake2b(topic_name).into();
        let topic_key = Key::Message(MessageAddr::new_topic_addr(entity_addr, topic_name_hash));

        // Check if the topic exists and get the summary.
        let Some(StoredValue::MessageTopic(prev_topic_summary)) =
            self.context.read_gs(&topic_key)? else {
            return Ok(Err(ApiError::MessageTopicNotRegistered));
        };

        let current_blocktime = self.context.get_blocktime();
        let message_index = if prev_topic_summary.blocktime() != current_blocktime {
            for index in 1..prev_topic_summary.message_count() {
                self.context
                    .prune_gs_unsafe(Key::message(entity_addr, topic_name_hash, index));
            }
            0
        } else {
            prev_topic_summary.message_count()
        };

        let Some(message_count) = message_index.checked_add(1) else {
            return Ok(Err(ApiError::MessageTopicFull));
        };
        let new_topic_summary = MessageTopicSummary::new(message_count, current_blocktime);

        let message_key = Key::message(entity_addr, topic_name_hash, message_index);
        let message_checksum = MessageChecksum(crypto::blake2b(
            message.to_bytes().map_err(Error::BytesRepr)?,
        ));

        self.context.metered_emit_message(
            topic_key,
            new_topic_summary,
            message_key,
            message_checksum,
            Message::new(
                entity_addr,
                message,
                topic_name.to_string(),
                topic_name_hash,
                message_index,
            ),
        )?;
        Ok(Ok(()))
    }
}

#[cfg(feature = "test-support")]
fn dump_runtime_stack_info(instance: casper_wasmi::ModuleRef, max_stack_height: u32) {
    let globals = instance.globals();
    let Some(current_runtime_call_stack_height) = globals.last()
    else {
        return;
    };

    if let RuntimeValue::I32(current_runtime_call_stack_height) =
        current_runtime_call_stack_height.get()
    {
        if current_runtime_call_stack_height > max_stack_height as i32 {
            eprintln!("runtime stack overflow, current={current_runtime_call_stack_height}, max={max_stack_height}");
        }
    };
}
