//! This module contains executor state of the WASM code.
mod args;
mod auction_internal;
mod externals;
mod handle_payment_internal;
mod host_function_flag;
mod mint_internal;
pub mod stack;
mod standard_payment_internal;
mod utils;

use std::{
    cmp,
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    iter::FromIterator,
};

use parity_wasm::elements::Module;
use tracing::error;
use wasmi::{MemoryRef, Trap, TrapKind};

use casper_types::{
    account::{Account, AccountHash, ActionType, Weight},
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contracts::{
        self, Contract, ContractPackage, ContractPackageStatus, ContractVersion, ContractVersions,
        DisabledVersions, EntryPoint, EntryPointAccess, EntryPoints, Group, Groups, NamedKeys,
        DEFAULT_ENTRY_POINT_NAME,
    },
    system::{
        self,
        auction::{self, EraInfo},
        handle_payment, mint, standard_payment, CallStackElement, SystemContractType, AUCTION,
        HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
    },
    AccessRights, ApiError, CLTyped, CLValue, ContextAccessRights, ContractHash,
    ContractPackageHash, ContractVersionKey, ContractWasm, DeployHash, EntryPointType, EraId, Gas,
    GrantedAccess, Key, NamedArg, Parameter, Phase, PublicKey, RuntimeArgs, StoredValue, Transfer,
    TransferResult, TransferredTo, URef, DICTIONARY_ITEM_KEY_MAX_LENGTH, U512,
};

use crate::{
    core::{
        engine_state::EngineConfig,
        execution::{self, Error},
        runtime::host_function_flag::HostFunctionFlag,
        runtime_context::{self, RuntimeContext},
        tracking_copy::TrackingCopyExt,
    },
    shared::{
        host_function_costs::{Cost, HostFunction},
        wasm_prep::{self, PreprocessingError},
    },
    storage::global_state::StateReader,
    system::{
        auction::Auction, handle_payment::HandlePayment, mint::Mint,
        standard_payment::StandardPayment,
    },
};
pub use stack::{RuntimeStack, RuntimeStackFrame, RuntimeStackOverflow};

/// Represents the runtime properties of a WASM execution.
pub struct Runtime<'a, R> {
    config: EngineConfig,
    memory: Option<MemoryRef>,
    module: Option<Module>,
    host_buffer: Option<CLValue>,
    context: RuntimeContext<'a, R>,
    stack: Option<RuntimeStack>,
    host_function_flag: HostFunctionFlag,
}

impl<'a, R> Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<Error>,
{
    /// Creates a new runtime instance.
    pub(crate) fn new(config: EngineConfig, context: RuntimeContext<'a, R>) -> Self {
        Runtime {
            config,
            memory: None,
            module: None,
            host_buffer: None,
            context,
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
            config: self.config,
            memory: Some(memory),
            module: Some(module),
            host_buffer: None,
            context,
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
            config: self.config,
            memory: None,
            module: None,
            host_buffer: None,
            context,
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
        if self.host_function_flag.is_in_host_function_scope()
            || self.is_system_immediate_caller()?
        {
            // This avoids charging the user in situation when the runtime is in the middle of
            // handling a host function call or a system contract calls other system contract.
            return Ok(());
        }
        self.context.charge_system_contract_call(amount)
    }

    /// Returns bytes from the WASM memory instance.
    fn bytes_from_mem(&self, ptr: u32, size: usize) -> Result<Vec<u8>, Error> {
        self.try_get_memory()?.get(ptr, size).map_err(Into::into)
    }

    /// Returns a deserialized type from the WASM memory instance.
    fn t_from_mem<T: FromBytes>(&self, ptr: u32, size: u32) -> Result<T, Error> {
        let bytes = self.bytes_from_mem(ptr, size as usize)?;
        bytesrepr::deserialize(bytes).map_err(Into::into)
    }

    /// Reads key (defined as `key_ptr` and `key_size` tuple) from Wasm memory.
    fn key_from_mem(&mut self, key_ptr: u32, key_size: u32) -> Result<Key, Error> {
        let bytes = self.bytes_from_mem(key_ptr, key_size as usize)?;
        bytesrepr::deserialize(bytes).map_err(Into::into)
    }

    /// Reads `CLValue` (defined as `cl_value_ptr` and `cl_value_size` tuple) from Wasm memory.
    fn cl_value_from_mem(
        &mut self,
        cl_value_ptr: u32,
        cl_value_size: u32,
    ) -> Result<CLValue, Error> {
        let bytes = self.bytes_from_mem(cl_value_ptr, cl_value_size as usize)?;
        bytesrepr::deserialize(bytes).map_err(Into::into)
    }

    /// Returns a deserialized string from the WASM memory instance.
    fn string_from_mem(&self, ptr: u32, size: u32) -> Result<String, Trap> {
        let bytes = self.bytes_from_mem(ptr, size as usize)?;
        bytesrepr::deserialize(bytes).map_err(|e| Error::BytesRepr(e).into())
    }

    fn get_module_from_entry_points(
        &mut self,
        entry_points: &EntryPoints,
    ) -> Result<Vec<u8>, Error> {
        let export_section = self
            .try_get_module()?
            .export_section()
            .ok_or_else(|| Error::FunctionNotFound(String::from("Missing Export Section")))?;

        let entry_point_names: Vec<&str> = entry_points.keys().map(|s| s.as_str()).collect();

        let maybe_missing_name: Option<String> = entry_point_names
            .iter()
            .find(|name| {
                !export_section
                    .entries()
                    .iter()
                    .any(|export_entry| export_entry.field() == **name)
            })
            .map(|s| String::from(*s));

        if let Some(missing_name) = maybe_missing_name {
            Err(Error::FunctionNotFound(missing_name))
        } else {
            let mut module = self.try_get_module()?.clone();
            pwasm_utils::optimize(&mut module, entry_point_names)?;
            parity_wasm::serialize(module).map_err(Error::ParityWasm)
        }
    }

    #[allow(clippy::wrong_self_convention)]
    fn is_valid_uref(&self, uref_ptr: u32, uref_size: u32) -> Result<bool, Trap> {
        let bytes = self.bytes_from_mem(uref_ptr, uref_size as usize)?;
        let uref: URef = bytesrepr::deserialize(bytes).map_err(Error::BytesRepr)?;
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
    fn get_immediate_caller(&self) -> Option<&CallStackElement> {
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

        let call_stack_cl_value = CLValue::from_t(call_stack.clone()).map_err(Error::CLValue)?;

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
        let memory = match self.try_get_memory() {
            Ok(memory) => memory,
            Err(error) => return Trap::from(error),
        };
        let mem_get = memory
            .get(value_ptr, value_size)
            .map_err(|e| Error::Interpreter(e.into()));
        match mem_get {
            Ok(buf) => {
                // Set the result field in the runtime and return the proper element of the `Error`
                // enum indicating that the reason for exiting the module was a call to ret.
                self.host_buffer = bytesrepr::deserialize(buf).ok();

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
    fn is_system_contract(&self, key: Key) -> Result<bool, Error> {
        let contract_hash = match key.into_hash() {
            Some(contract_hash_bytes) => ContractHash::new(contract_hash_bytes),
            None => return Ok(false),
        };

        self.context.is_system_contract(&contract_hash)
    }

    /// Checks if current context is the mint system contract.
    pub(crate) fn is_mint(&self, key: Key) -> bool {
        let hash = match self.context.get_system_contract(MINT) {
            Ok(hash) => hash,
            Err(_) => {
                error!("Failed to get system mint contract hash");
                return false;
            }
        };
        key.into_hash() == Some(hash.value())
    }

    /// Checks if current context is the `handle_payment` system contract.
    pub(crate) fn is_handle_payment(&self, key: Key) -> bool {
        let hash = match self.context.get_system_contract(HANDLE_PAYMENT) {
            Ok(hash) => hash,
            Err(_) => {
                error!("Failed to get system handle payment contract hash");
                return false;
            }
        };
        key.into_hash() == Some(hash.value())
    }

    /// Checks if current context is the auction system contract.
    pub(crate) fn is_auction(&self, key: Key) -> bool {
        let hash = match self.context.get_system_contract(AUCTION) {
            Ok(hash) => hash,
            Err(_) => {
                error!("Failed to get system auction contract hash");
                return false;
            }
        };
        key.into_hash() == Some(hash.value())
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
        let base_key = Key::from(mint_hash);
        let mint_contract = self
            .context
            .state()
            .borrow_mut()
            .get_contract(self.context.correlation_id(), mint_hash)?;
        let mut named_keys = mint_contract.named_keys().to_owned();

        let runtime_context = self.context.new_from_self(
            base_key,
            EntryPointType::Contract,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut mint_runtime = self.new_with_stack(runtime_context, stack);

        let system_config = self.config.system_config();
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
                mint_runtime.charge_system_contract_call(mint_costs.mint)?;

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
        let base_key = Key::from(handle_payment_hash);
        let handle_payment_contract = self
            .context
            .state()
            .borrow_mut()
            .get_contract(self.context.correlation_id(), handle_payment_hash)?;
        let mut named_keys = handle_payment_contract.named_keys().to_owned();

        let runtime_context = self.context.new_from_self(
            base_key,
            EntryPointType::Contract,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut runtime = self.new_with_stack(runtime_context, stack);

        let system_config = self.config.system_config();
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

    /// Calls host standard payment contract.
    pub(crate) fn call_host_standard_payment(&mut self, stack: RuntimeStack) -> Result<(), Error> {
        // NOTE: This method (unlike other call_host_* methods) already runs on its own runtime
        // context.
        self.stack = Some(stack);
        let gas_counter = self.gas_counter();
        let amount: U512 =
            Self::get_named_argument(self.context.args(), standard_payment::ARG_AMOUNT)?;
        let result = self.pay(amount).map_err(Self::reverter);
        self.set_gas_counter(gas_counter);
        result
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
        let base_key = Key::from(auction_hash);
        let auction_contract = self
            .context
            .state()
            .borrow_mut()
            .get_contract(self.context.correlation_id(), auction_hash)?;
        let mut named_keys = auction_contract.named_keys().to_owned();

        let runtime_context = self.context.new_from_self(
            base_key,
            EntryPointType::Contract,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut runtime = self.new_with_stack(runtime_context, stack);

        let system_config = self.config.system_config();
        let auction_costs = system_config.auction_costs();

        let result = match entry_point_name {
            auction::METHOD_GET_ERA_VALIDATORS => (|| {
                runtime.charge_system_contract_call(auction_costs.get_era_validators)?;

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

                let minimum_delegation_amount = self.config.minimum_delegation_amount();

                let result = runtime
                    .delegate(delegator, validator, amount, minimum_delegation_amount)
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
                runtime.charge_system_contract_call(auction_costs.undelegate)?;

                let delegator = Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR)?;
                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
                let new_validator =
                    Self::get_named_argument(runtime_args, auction::ARG_NEW_VALIDATOR)?;

                let minimum_delegation_amount = self.config.minimum_delegation_amount();

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

                runtime
                    .run_auction(era_end_timestamp_millis, evicted_validators)
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn slash(validator_account_hashes: &[AccountHash]) -> Result<(), Error>`
            auction::METHOD_SLASH => (|| {
                runtime.charge_system_contract_call(auction_costs.slash)?;

                let validator_public_keys =
                    Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR_PUBLIC_KEYS)?;
                runtime
                    .slash(validator_public_keys)
                    .map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn distribute(reward_factors: BTreeMap<PublicKey, u64>) -> Result<(), Error>`
            auction::METHOD_DISTRIBUTE => (|| {
                runtime.charge_system_contract_call(auction_costs.distribute)?;

                let reward_factors: BTreeMap<PublicKey, u64> =
                    Self::get_named_argument(runtime_args, auction::ARG_REWARD_FACTORS)?;
                runtime.distribute(reward_factors).map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn read_era_id() -> Result<EraId, Error>`
            auction::METHOD_READ_ERA_ID => (|| {
                runtime.charge_system_contract_call(auction_costs.read_era_id)?;

                let result = runtime.read_era_id().map_err(Self::reverter)?;
                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_ACTIVATE_BID => (|| {
                runtime.charge_system_contract_call(auction_costs.read_era_id)?;

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
        contract_hash: ContractHash,
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
        let wasm_config = self.config.wasm_config();
        let module = wasm_prep::preprocess(*wasm_config, module_bytes)?;
        let (instance, memory) =
            utils::instance_and_memory(module.clone(), protocol_version, wasm_config)?;
        self.memory = Some(memory);
        self.module = Some(module);
        self.stack = Some(stack);
        self.context.set_args(utils::attenuate_uref_in_args(
            self.context.args().clone(),
            self.context.account().main_purse().addr(),
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
        contract_hash: ContractHash,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Result<CLValue, Error> {
        let key = contract_hash.into();
        let contract = match self.context.read_gs(&key)? {
            Some(StoredValue::Contract(contract)) => contract,
            Some(_) => {
                return Err(Error::InvalidContract(contract_hash));
            }
            None => return Err(Error::KeyNotFound(key)),
        };

        let contract_entry_point = contract
            .entry_point(entry_point_name)
            .cloned()
            .ok_or_else(|| Error::NoSuchMethod(entry_point_name.to_owned()))?;

        let contract_package_hash = contract.contract_package_hash();

        let contract_package = match self.context.read_gs(&contract_package_hash.into())? {
            Some(StoredValue::ContractPackage(contract_package)) => contract_package,
            Some(_) => {
                return Err(Error::InvalidContractPackage(contract_package_hash));
            }
            None => return Err(Error::KeyNotFound(key)),
        };

        self.execute_contract(
            contract_package,
            contract_hash,
            contract,
            contract_entry_point,
            args,
        )
    }

    /// Calls `version` of the contract living at `key`, invoking `method` with
    /// supplied `args`. This function also checks the args conform with the
    /// types given in the contract header.
    pub fn call_versioned_contract(
        &mut self,
        contract_package_hash: ContractPackageHash,
        contract_version: Option<ContractVersion>,
        entry_point_name: String,
        args: RuntimeArgs,
    ) -> Result<CLValue, Error> {
        let contract_package_key = contract_package_hash.into();
        let contract_package = match self.context.read_gs(&contract_package_key)? {
            Some(StoredValue::ContractPackage(contract_package)) => contract_package,
            Some(_) => {
                return Err(Error::InvalidContractPackage(contract_package_hash));
            }
            None => return Err(Error::KeyNotFound(contract_package_key)),
        };

        let contract_version_key = match contract_version {
            Some(version) => {
                ContractVersionKey::new(self.context.protocol_version().value().major, version)
            }
            None => match contract_package.current_contract_version() {
                Some(v) => v,
                None => return Err(Error::NoActiveContractVersions(contract_package_hash)),
            },
        };

        // Get contract entry point hash
        let contract_hash = contract_package
            .lookup_contract_hash(contract_version_key)
            .cloned()
            .ok_or(Error::InvalidContractVersion(contract_version_key))?;

        // Get contract data
        let contract_key = contract_hash.into();
        let contract = match self.context.read_gs(&contract_key)? {
            Some(StoredValue::Contract(contract)) => contract,
            Some(_) => {
                return Err(Error::InvalidContract(contract_hash));
            }
            None => return Err(Error::KeyNotFound(contract_key)),
        };

        let contract_entry_point = contract
            .entry_point(&entry_point_name)
            .cloned()
            .ok_or_else(|| Error::NoSuchMethod(entry_point_name.to_owned()))?;

        self.execute_contract(
            contract_package,
            contract_hash,
            contract,
            contract_entry_point,
            args,
        )
    }

    fn get_context_key_for_contract_call(
        &self,
        contract_hash: ContractHash,
        entry_point: &EntryPoint,
    ) -> Result<Key, Error> {
        let current = self.context.entry_point_type();
        let next = entry_point.entry_point_type();
        match (current, next) {
            (EntryPointType::Contract, EntryPointType::Session) => {
                // Session code can't be called from Contract code for security reasons.
                Err(Error::InvalidContext)
            }
            (EntryPointType::Session, EntryPointType::Session) => {
                // Session code called from session reuses current base key
                Ok(self.context.base_key())
            }
            (EntryPointType::Session, EntryPointType::Contract)
            | (EntryPointType::Contract, EntryPointType::Contract) => Ok(contract_hash.into()),
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
        contract_package: ContractPackage,
        contract_hash: ContractHash,
        contract: Contract,
        entry_point: EntryPoint,
        args: RuntimeArgs,
    ) -> Result<CLValue, Error> {
        // if public, allowed
        // if not public, restricted to user group access
        self.validate_group_membership(&contract_package, entry_point.access())?;

        if self.config.strict_argument_checking() {
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
        // if session the caller's context
        // else the called contract's context
        let context_key = self.get_context_key_for_contract_call(contract_hash, &entry_point)?;
        let protocol_version = self.context.protocol_version();

        // Check for major version compatibility before calling
        if !contract.is_compatible_protocol_version(protocol_version) {
            return Err(Error::IncompatibleProtocolMajorVersion {
                actual: contract.protocol_version().value().major,
                expected: protocol_version.value().major,
            });
        }

        let (mut named_keys, mut access_rights) = match entry_point.entry_point_type() {
            EntryPointType::Session => (
                self.context.account().named_keys().clone(),
                self.context.account().extract_access_rights(),
            ),
            EntryPointType::Contract => (
                contract.named_keys().clone(),
                contract.extract_access_rights(contract_hash),
            ),
        };

        let stack = {
            let mut stack = self.try_get_stack()?.clone();

            let call_stack_element = match entry_point.entry_point_type() {
                EntryPointType::Session => CallStackElement::stored_session(
                    self.context.account().account_hash(),
                    contract.contract_package_hash(),
                    contract_hash,
                ),
                EntryPointType::Contract => CallStackElement::stored_contract(
                    contract.contract_package_hash(),
                    contract_hash,
                ),
            };
            stack.push(call_stack_element)?;

            stack
        };

        // Determines if this call originated from the system account based on a first
        // element of the call stack.
        let is_system_account = self.context.get_caller() == PublicKey::System.to_account_hash();
        // Is the immediate caller a system contract, such as when the auction calls the mint.
        let is_caller_system_contract =
            self.is_system_contract(self.context.access_rights().context_key())?;
        // Checks if the contract we're about to call is a system contract.
        let is_calling_system_contract = self.is_system_contract(context_key)?;
        // uref attenuation is necessary in the following circumstances:
        //   the originating account (aka the caller) is not the system account and
        //   the immediate caller is either a normal account or a normal contract and
        //   the target contract about to be called is a normal contract
        let should_attenuate_urefs =
            !is_system_account && !is_caller_system_contract && !is_calling_system_contract;

        let context_args = if should_attenuate_urefs {
            // Main purse URefs should be attenuated only when a non-system contract is executed by
            // a non-system account to avoid possible phishing attack scenarios.
            utils::attenuate_uref_in_args(
                args,
                self.context.account().main_purse().addr(),
                AccessRights::WRITE,
            )?
        } else {
            args
        };

        let extended_access_rights = {
            let mut all_urefs = vec![];
            for arg in context_args.to_values() {
                let urefs = utils::extract_urefs(arg)?;
                if !is_caller_system_contract || !is_calling_system_contract {
                    for uref in &urefs {
                        self.context.validate_uref(uref)?;
                    }
                }
                all_urefs.extend(urefs);
            }
            all_urefs
        };

        access_rights.extend(&extended_access_rights);

        if self.is_mint(context_key) {
            return self.call_host_mint(entry_point.name(), &context_args, access_rights, stack);
        } else if self.is_handle_payment(context_key) {
            return self.call_host_handle_payment(
                entry_point.name(),
                &context_args,
                access_rights,
                stack,
            );
        } else if self.is_auction(context_key) {
            return self.call_host_auction(entry_point.name(), &context_args, access_rights, stack);
        }

        let module: Module = {
            let wasm_key = contract.contract_wasm_key();

            let contract_wasm: ContractWasm = match self.context.read_gs(&wasm_key)? {
                Some(StoredValue::ContractWasm(contract_wasm)) => contract_wasm,
                Some(_) => return Err(Error::InvalidContractWasm(contract.contract_wasm_hash())),
                None => return Err(Error::KeyNotFound(context_key)),
            };

            parity_wasm::deserialize_buffer(contract_wasm.bytes())?
        };

        let context = self.context.new_from_self(
            context_key,
            entry_point.entry_point_type(),
            &mut named_keys,
            access_rights,
            context_args,
        );
        let protocol_version = self.context.protocol_version();
        let (instance, memory) = utils::instance_and_memory(
            module.clone(),
            protocol_version,
            self.config.wasm_config(),
        )?;
        let runtime = &mut Runtime::new_invocation_runtime(self, context, module, memory, stack);

        let result = instance.invoke_export(entry_point.name(), &[], runtime);

        // The `runtime`'s context was initialized with our counter from before the call and any gas
        // charged by the sub-call was added to its counter - so let's copy the correct value of the
        // counter from there to our counter.
        self.context.set_gas_counter(runtime.context.gas_counter());

        {
            let transfers = self.context.transfers_mut();
            *transfers = runtime.context.transfers().to_owned();
        }

        let error = match result {
            Err(error) => error,
            // If `Ok` and the `host_buffer` is `None`, the contract's execution succeeded but did
            // not explicitly call `runtime::ret()`.  Treat as though the execution returned the
            // unit type `()` as per Rust functions which don't specify a return value.
            Ok(_) => {
                if self.context.entry_point_type() == EntryPointType::Session
                    && runtime.context.entry_point_type() == EntryPointType::Session
                {
                    // Overwrites parent's named keys with child's new named key but only when
                    // running session code.
                    *self.context.named_keys_mut() = runtime.context.named_keys().clone();
                }
                self.context
                    .set_remaining_spending_limit(runtime.context.remaining_spending_limit());
                return Ok(runtime.take_host_buffer().unwrap_or(CLValue::from_t(())?));
            }
        };

        if let Some(host_error) = error.as_host_error() {
            // If the "error" was in fact a trap caused by calling `ret` then this is normal
            // operation and we should return the value captured in the Runtime result field.
            let downcasted_error = host_error.downcast_ref::<Error>();
            match downcasted_error {
                Some(Error::Ret(ref ret_urefs)) => {
                    // Insert extra urefs returned from call.
                    // Those returned URef's are guaranteed to be valid as they were already
                    // validated in the `ret` call inside context we ret from.
                    self.context.access_rights_extend(ret_urefs);

                    if self.context.entry_point_type() == EntryPointType::Session
                        && runtime.context.entry_point_type() == EntryPointType::Session
                    {
                        // Overwrites parent's named keys with child's new named keys but only when
                        // running session code.
                        *self.context.named_keys_mut() = runtime.context.named_keys().clone();
                    }

                    // Stored contracts are expected to always call a `ret` function, otherwise it's
                    // an error.
                    return runtime.take_host_buffer().ok_or(Error::ExpectedReturnValue);
                }
                Some(error) => return Err(error.clone()),
                None => return Err(Error::Interpreter(host_error.to_string())),
            }
        }

        Err(Error::Interpreter(error.into()))
    }

    fn call_contract_host_buffer(
        &mut self,
        contract_hash: ContractHash,
        entry_point_name: &str,
        args_bytes: Vec<u8>,
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        // Exit early if the host buffer is already occupied
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        let args: RuntimeArgs = bytesrepr::deserialize(args_bytes)?;
        let result = self.call_contract(contract_hash, entry_point_name, args)?;
        self.manage_call_contract_host_buffer(result_size_ptr, result)
    }

    fn call_versioned_contract_host_buffer(
        &mut self,
        contract_package_hash: ContractPackageHash,
        contract_version: Option<ContractVersion>,
        entry_point_name: String,
        args_bytes: Vec<u8>,
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        // Exit early if the host buffer is already occupied
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        let args: RuntimeArgs = bytesrepr::deserialize(args_bytes)?;
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
        is_locked: ContractPackageStatus,
    ) -> Result<(ContractPackage, URef), Error> {
        let access_key = self.context.new_unit_uref()?;
        let contract_package = ContractPackage::new(
            access_key,
            ContractVersions::default(),
            DisabledVersions::default(),
            Groups::default(),
            is_locked,
        );

        Ok((contract_package, access_key))
    }

    fn create_contract_package_at_hash(
        &mut self,
        lock_status: ContractPackageStatus,
    ) -> Result<([u8; 32], [u8; 32]), Error> {
        let addr = self.context.new_hash_address()?;
        let (contract_package, access_key) = self.create_contract_package(lock_status)?;
        self.context
            .metered_write_gs_unsafe(Key::Hash(addr), contract_package)?;
        Ok((addr, access_key.addr()))
    }

    fn create_contract_user_group(
        &mut self,
        contract_package_hash: ContractPackageHash,
        label: String,
        num_new_urefs: u32,
        mut existing_urefs: BTreeSet<URef>,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        let mut contract_package: ContractPackage = self
            .context
            .get_validated_contract_package(contract_package_hash)?;

        let groups = contract_package.groups_mut();
        let new_group = Group::new(label);

        // Ensure group does not already exist
        if groups.get(&new_group).is_some() {
            return Ok(Err(contracts::Error::GroupAlreadyExists.into()));
        }

        // Ensure there are not too many groups
        if groups.len() >= (contracts::MAX_GROUPS as usize) {
            return Ok(Err(contracts::Error::MaxGroupsExceeded.into()));
        }

        // Ensure there are not too many urefs
        let total_urefs: usize = groups.values().map(|urefs| urefs.len()).sum::<usize>()
            + (num_new_urefs as usize)
            + existing_urefs.len();
        if total_urefs > contracts::MAX_TOTAL_UREFS {
            let err = contracts::Error::MaxTotalURefsExceeded;
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

    #[allow(clippy::too_many_arguments)]
    fn add_contract_version(
        &mut self,
        contract_package_hash: ContractPackageHash,
        entry_points: EntryPoints,
        mut named_keys: NamedKeys,
        output_ptr: u32,
        output_size: usize,
        bytes_written_ptr: u32,
        version_ptr: u32,
    ) -> Result<Result<(), ApiError>, Error> {
        self.context
            .validate_key(&Key::from(contract_package_hash))?;

        let mut contract_package: ContractPackage = self
            .context
            .get_validated_contract_package(contract_package_hash)?;

        let version = contract_package.current_contract_version();

        // Return an error if the contract is locked and has some version associated with it.
        if contract_package.is_locked() && version.is_some() {
            return Err(Error::LockedContract(contract_package_hash));
        }

        let contract_wasm_hash = self.context.new_hash_address()?;
        let contract_wasm = {
            let module_bytes = self.get_module_from_entry_points(&entry_points)?;
            ContractWasm::new(module_bytes)
        };

        let contract_hash = self.context.new_hash_address()?;

        let protocol_version = self.context.protocol_version();
        let major = protocol_version.value().major;

        // TODO: EE-1032 - Implement different ways of carrying on existing named keys
        if let Some(previous_contract_hash) = contract_package.current_contract_hash() {
            let previous_contract: Contract =
                self.context.read_gs_typed(&previous_contract_hash.into())?;

            let mut previous_named_keys = previous_contract.take_named_keys();
            named_keys.append(&mut previous_named_keys);
        }

        let contract = Contract::new(
            contract_package_hash,
            contract_wasm_hash.into(),
            named_keys,
            entry_points,
            protocol_version,
        );

        let insert_contract_result =
            contract_package.insert_contract_version(major, contract_hash.into());

        self.context
            .metered_write_gs_unsafe(Key::Hash(contract_wasm_hash), contract_wasm)?;
        self.context
            .metered_write_gs_unsafe(Key::Hash(contract_hash), contract)?;
        self.context
            .metered_write_gs_unsafe(contract_package_hash, contract_package)?;

        // return contract key to caller
        {
            let key_bytes = match contract_hash.to_bytes() {
                Ok(bytes) => bytes,
                Err(error) => return Ok(Err(error.into())),
            };

            // `output_size` must be >= actual length of serialized Key bytes
            if output_size < key_bytes.len() {
                return Ok(Err(ApiError::BufferTooSmall));
            }

            // Set serialized Key bytes into the output buffer
            if let Err(error) = self.try_get_memory()?.set(output_ptr, &key_bytes) {
                return Err(Error::Interpreter(error.into()));
            }

            // SAFETY: For all practical purposes following conversion is assumed to be safe
            let bytes_size: u32 = key_bytes
                .len()
                .try_into()
                .expect("Serialized value should fit within the limit");
            let size_bytes = bytes_size.to_le_bytes(); // Wasm is little-endian
            if let Err(error) = self.try_get_memory()?.set(bytes_written_ptr, &size_bytes) {
                return Err(Error::Interpreter(error.into()));
            }

            let version_value: u32 = insert_contract_result.contract_version();
            let version_bytes = version_value.to_le_bytes();
            if let Err(error) = self.try_get_memory()?.set(version_ptr, &version_bytes) {
                return Err(Error::Interpreter(error.into()));
            }
        }

        Ok(Ok(()))
    }

    fn disable_contract_version(
        &mut self,
        contract_package_hash: ContractPackageHash,
        contract_hash: ContractHash,
    ) -> Result<Result<(), ApiError>, Error> {
        let contract_package_key = contract_package_hash.into();
        self.context.validate_key(&contract_package_key)?;

        let mut contract_package: ContractPackage = self
            .context
            .get_validated_contract_package(contract_package_hash)?;

        // Return an error in trying to disable the (singular) version of a locked contract.
        if contract_package.is_locked() {
            return Err(Error::LockedContract(contract_package_hash));
        }

        if let Err(err) = contract_package.disable_contract_version(contract_hash) {
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
        if self.context.base_key() != Key::from(self.context.get_system_contract(MINT)?) {
            return Err(Error::InvalidContext);
        }

        if self.context.phase() != Phase::Session {
            return Ok(());
        }

        let transfer_addr = self.context.new_transfer_addr()?;
        let transfer = {
            let deploy_hash: DeployHash = self.context.get_deploy_hash();
            let from: AccountHash = self.context.account().account_hash();
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
    fn record_era_info(&mut self, era_id: EraId, era_info: EraInfo) -> Result<(), Error> {
        if self.context.base_key() != Key::from(self.context.get_system_contract(AUCTION)?) {
            return Err(Error::InvalidContext);
        }

        if self.context.phase() != Phase::Session {
            return Ok(());
        }

        self.context.write_era_info(Key::EraInfo(era_id), era_info);

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
                bytesrepr::deserialize(source_serialized).map_err(Error::BytesRepr)?;
            source
        };
        let weight = Weight::new(weight_value);

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
                bytesrepr::deserialize(source_serialized).map_err(Error::BytesRepr)?;
            source
        };
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
                bytesrepr::deserialize(source_serialized).map_err(Error::BytesRepr)?;
            source
        };
        let weight = Weight::new(weight_value);

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
        match ActionType::try_from(action_type_value) {
            Ok(action_type) => {
                let threshold = Weight::new(threshold_value);
                match self.context.set_action_threshold(action_type, threshold) {
                    Ok(_) => Ok(0),
                    Err(Error::SetThresholdFailure(e)) => Ok(e as i32),
                    Err(e) => Err(e.into()),
                }
            }
            Err(_) => Err(Trap::new(TrapKind::Unreachable)),
        }
    }

    /// Looks up the public mint contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_mint_contract(&self) -> Result<ContractHash, Error> {
        self.context.get_system_contract(MINT)
    }

    /// Looks up the public handle payment contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_handle_payment_contract(&self) -> Result<ContractHash, Error> {
        self.context.get_system_contract(HANDLE_PAYMENT)
    }

    /// Looks up the public standard payment contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_standard_payment_contract(&self) -> Result<ContractHash, Error> {
        self.context.get_system_contract(STANDARD_PAYMENT)
    }

    /// Looks up the public auction contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_auction_contract(&self) -> Result<ContractHash, Error> {
        self.context.get_system_contract(AUCTION)
    }

    /// Calls the `read_base_round_reward` method on the mint contract at the given mint
    /// contract key
    fn mint_read_base_round_reward(
        &mut self,
        mint_contract_hash: ContractHash,
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
    fn mint_mint(&mut self, mint_contract_hash: ContractHash, amount: U512) -> Result<URef, Error> {
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
        mint_contract_hash: ContractHash,
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
    fn mint_create(&mut self, mint_contract_hash: ContractHash) -> Result<URef, Error> {
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
        mint_contract_hash: ContractHash,
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

        let target_key = Key::Account(target);

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
                let account = Account::create(target, Default::default(), target_purse);
                self.context.write_account(target_key, account)?;
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
        self.transfer_from_purse_to_account(source, target, amount, id)
    }

    /// Transfers `amount` of motes from `source` purse to `target` account.
    /// If that account does not exist, creates one.
    fn transfer_from_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
        id: Option<u64>,
    ) -> Result<TransferResult, Error> {
        let _scoped_host_function_flag = self.host_function_flag.enter_host_function_scope();

        let target_key = Key::Account(target);
        // Look up the account at the given public key's address
        match self.context.read_account(&target_key)? {
            None => {
                // If no account exists, create a new account and transfer the amount to its
                // purse.
                self.transfer_to_new_account(source, target, amount, id)
            }
            Some(StoredValue::Account(account)) => {
                // Attenuate the target main purse
                let target_uref = account.main_purse_add_only();

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
            Some(_) => {
                // If some other value exists, return an error
                Err(Error::AccountNotFound(target_key))
            }
        }
    }

    /// Transfers `amount` of motes from `source` purse to `target` purse.
    #[allow(clippy::too_many_arguments)]
    fn transfer_from_purse_to_purse(
        &mut self,
        source_ptr: u32,
        source_size: u32,
        target_ptr: u32,
        target_size: u32,
        amount_ptr: u32,
        amount_size: u32,
        id_ptr: u32,
        id_size: u32,
    ) -> Result<Result<(), mint::Error>, Error> {
        let source: URef = {
            let bytes = self.bytes_from_mem(source_ptr, source_size as usize)?;
            bytesrepr::deserialize(bytes).map_err(Error::BytesRepr)?
        };

        let target: URef = {
            let bytes = self.bytes_from_mem(target_ptr, target_size as usize)?;
            bytesrepr::deserialize(bytes).map_err(Error::BytesRepr)?
        };

        let amount: U512 = {
            let bytes = self.bytes_from_mem(amount_ptr, amount_size as usize)?;
            bytesrepr::deserialize(bytes).map_err(Error::BytesRepr)?
        };

        let id: Option<u64> = {
            let bytes = self.bytes_from_mem(id_ptr, id_size as usize)?;
            bytesrepr::deserialize(bytes).map_err(Error::BytesRepr)?
        };

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
            match bytesrepr::deserialize(bytes) {
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
        let contract_hash: ContractHash = match SystemContractType::try_from(system_contract_index)
        {
            Ok(SystemContractType::Mint) => self.get_mint_contract()?,
            Ok(SystemContractType::HandlePayment) => self.get_handle_payment_contract()?,
            Ok(SystemContractType::StandardPayment) => self.get_standard_payment_contract()?,
            Ok(SystemContractType::Auction) => self.get_auction_contract()?,
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
    fn validate_group_membership(
        &self,
        package: &ContractPackage,
        access: &EntryPointAccess,
    ) -> Result<(), Error> {
        runtime_context::validate_group_membership(package, access, |uref| {
            self.context.validate_uref(uref).is_ok()
        })
    }

    /// Remove a user group from access to a contract
    fn remove_contract_user_group(
        &mut self,
        package_key: ContractPackageHash,
        label: Group,
    ) -> Result<Result<(), ApiError>, Error> {
        let mut package: ContractPackage =
            self.context.get_validated_contract_package(package_key)?;

        let group_to_remove = Group::new(label);
        let groups = package.groups_mut();

        // Ensure group exists in groups
        if groups.get(&group_to_remove).is_none() {
            return Ok(Err(contracts::Error::GroupDoesNotExist.into()));
        }

        // Remove group if it is not referenced by at least one entry_point in active versions.
        let versions = package.versions();
        for contract_hash in versions.values() {
            let entry_points = {
                let contract: Contract = self.context.read_gs_typed(&Key::from(*contract_hash))?;
                contract.entry_points().clone().take_entry_points()
            };
            for entry_point in entry_points {
                match entry_point.access() {
                    EntryPointAccess::Public => {
                        continue;
                    }
                    EntryPointAccess::Groups(groups) => {
                        if groups.contains(&group_to_remove) {
                            return Ok(Err(contracts::Error::GroupInUse.into()));
                        }
                    }
                }
            }
        }

        if !package.remove_group(&group_to_remove) {
            return Ok(Err(contracts::Error::GroupInUse.into()));
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
        let mut contract_package = self
            .context
            .get_validated_contract_package(contract_package_hash)?;
        let groups = contract_package.groups_mut();

        let group_label = Group::new(label);

        // Ensure there are not too many urefs
        let total_urefs: usize = groups.values().map(|urefs| urefs.len()).sum();

        if total_urefs + 1 > contracts::MAX_TOTAL_UREFS {
            return Ok(Err(contracts::Error::MaxTotalURefsExceeded.into()));
        }

        // Ensure given group exists and does not exceed limits
        let group = match groups.get_mut(&group_label) {
            Some(group) if group.len() + 1 > contracts::MAX_GROUPS as usize => {
                // Ensures there are not too many groups to fit in amount of new urefs
                return Ok(Err(contracts::Error::MaxTotalURefsExceeded.into()));
            }
            Some(group) => group,
            None => return Ok(Err(contracts::Error::GroupDoesNotExist.into())),
        };

        // Proceed with creating new URefs
        let new_uref = self.context.new_unit_uref()?;
        if !group.insert(new_uref) {
            return Ok(Err(contracts::Error::URefAlreadyExists.into()));
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
        let contract_package_hash: ContractPackageHash =
            self.t_from_mem(package_ptr, package_size)?;
        let label: String = self.t_from_mem(label_ptr, label_size)?;
        let urefs: BTreeSet<URef> = self.t_from_mem(urefs_ptr, urefs_size)?;

        let mut contract_package = self
            .context
            .get_validated_contract_package(contract_package_hash)?;

        let groups = contract_package.groups_mut();
        let group_label = Group::new(label);

        let group = match groups.get_mut(&group_label) {
            Some(group) => group,
            None => return Ok(Err(contracts::Error::GroupDoesNotExist.into())),
        };

        if urefs.is_empty() {
            return Ok(Ok(()));
        }

        for uref in urefs {
            if !group.remove(&uref) {
                return Ok(Err(contracts::Error::UnableToRemoveURef.into()));
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
        T: AsRef<[Cost]> + Copy,
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
        let dictionary_item_key_bytes = self.bytes_from_mem(
            dictionary_item_key_bytes_ptr,
            dictionary_item_key_bytes_size as usize,
        )?;

        let dictionary_item_key = if let Ok(item_key) = String::from_utf8(dictionary_item_key_bytes)
        {
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
        let dictionary_item_key_bytes = self.bytes_from_mem(key_ptr, key_size as usize)?;
        if dictionary_item_key_bytes.len() > DICTIONARY_ITEM_KEY_MAX_LENGTH {
            return Ok(Err(ApiError::DictionaryItemKeyExceedsLength));
        }
        let dictionary_item_key = if let Ok(item_key) = String::from_utf8(dictionary_item_key_bytes)
        {
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
            CallStackElement::StoredSession { contract_hash, .. }
            | CallStackElement::StoredContract { contract_hash, .. } => {
                Ok(self.context.is_system_contract(contract_hash)?)
            }
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
}
