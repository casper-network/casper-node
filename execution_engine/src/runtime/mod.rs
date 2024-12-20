//! This module contains executor state of the WASM code.
mod args;
mod auction_internal;
pub mod cryptography;
mod externals;
mod handle_payment_internal;
mod host_function_flag;
mod mint_internal;
pub mod stack;
mod utils;
pub(crate) mod wasm_prep;

use std::{
    cmp,
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    iter::FromIterator,
};

use casper_wasm::elements::Module;
use casper_wasmi::{MemoryRef, Trap, TrapCode};
use tracing::{debug, error, warn};

#[cfg(feature = "test-support")]
use casper_wasmi::RuntimeValue;
use itertools::Itertools;
use num_rational::Ratio;

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::{auction::Auction, handle_payment::HandlePayment, mint::Mint},
    tracking_copy::TrackingCopyExt,
};
use casper_types::{
    account::{
        Account, AccountHash, AddKeyFailure, RemoveKeyFailure, SetThresholdFailure,
        UpdateKeyFailure,
    },
    addressable_entity::{
        self, ActionThresholds, ActionType, AddressableEntity, AddressableEntityHash,
        AssociatedKeys, ContractRuntimeTag, EntityKindTag, EntryPoint, EntryPointAccess,
        EntryPointType, EntryPoints, MessageTopicError, MessageTopics, NamedKeyAddr, NamedKeyValue,
        Parameter, Weight, DEFAULT_ENTRY_POINT_NAME,
    },
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contract_messages::{
        Message, MessageAddr, MessagePayload, MessageTopicOperation, MessageTopicSummary,
    },
    contracts::{
        ContractHash, ContractPackage, ContractPackageHash, ContractPackageStatus,
        ContractVersions, DisabledVersions, NamedKeys,
    },
    system::{
        self,
        auction::{self, DelegatorKind, EraInfo},
        handle_payment, mint, CallStackElement, Caller, CallerInfo, SystemEntityType, AUCTION,
        HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
    },
    AccessRights, ApiError, BlockGlobalAddr, BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash,
    ByteCodeKind, CLTyped, CLValue, ContextAccessRights, Contract, ContractWasm, EntityAddr,
    EntityKind, EntityVersion, EntityVersionKey, EntityVersions, Gas, GrantedAccess, Group, Groups,
    HashAddr, HostFunction, HostFunctionCost, InitiatorAddr, Key, NamedArg, Package, PackageHash,
    PackageStatus, Phase, PublicKey, RuntimeArgs, RuntimeFootprint, StoredValue, Transfer,
    TransferResult, TransferV2, TransferredTo, URef, DICTIONARY_ITEM_KEY_MAX_LENGTH, U512,
};

use crate::{
    execution::ExecError, runtime::host_function_flag::HostFunctionFlag,
    runtime_context::RuntimeContext,
};
pub use stack::{RuntimeStack, RuntimeStackFrame, RuntimeStackOverflow};
pub use wasm_prep::{
    cycles_for_instruction, preprocess, PreprocessingError, WasmValidationError,
    DEFAULT_BR_TABLE_MAX_SIZE, DEFAULT_MAX_GLOBALS, DEFAULT_MAX_PARAMETER_COUNT,
    DEFAULT_MAX_TABLE_SIZE,
};

#[derive(Debug)]
enum CallContractIdentifier {
    Contract {
        contract_hash: HashAddr,
    },
    ContractPackage {
        contract_package_hash: HashAddr,
        version: Option<EntityVersion>,
    },
}

#[repr(u8)]
enum CallerInformation {
    Initiator = 0,
    Immediate = 1,
    FullCallChain = 2,
}

impl TryFrom<u8> for CallerInformation {
    type Error = ApiError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CallerInformation::Initiator),
            1 => Ok(CallerInformation::Immediate),
            2 => Ok(CallerInformation::FullCallChain),
            _ => Err(ApiError::InvalidCallerInfoRequest),
        }
    }
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

    fn gas(&mut self, amount: Gas) -> Result<(), ExecError> {
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
    pub(crate) fn charge_system_contract_call<T>(&mut self, amount: T) -> Result<(), ExecError>
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
    ) -> Result<Ret, ExecError> {
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
    fn bytes_from_mem(&self, ptr: u32, size: usize) -> Result<Vec<u8>, ExecError> {
        self.checked_memory_slice(ptr as usize, size, |data| data.to_vec())
    }

    /// Returns a deserialized type from the WASM memory instance.
    #[inline]
    fn t_from_mem<T: FromBytes>(&self, ptr: u32, size: u32) -> Result<T, ExecError> {
        let result = self.checked_memory_slice(ptr as usize, size as usize, |data| {
            bytesrepr::deserialize_from_slice(data)
        })?;
        Ok(result?)
    }

    /// Reads key (defined as `key_ptr` and `key_size` tuple) from Wasm memory.
    #[inline]
    fn key_from_mem(&mut self, key_ptr: u32, key_size: u32) -> Result<Key, ExecError> {
        self.t_from_mem(key_ptr, key_size)
    }

    /// Reads `CLValue` (defined as `cl_value_ptr` and `cl_value_size` tuple) from Wasm memory.
    #[inline]
    fn cl_value_from_mem(
        &mut self,
        cl_value_ptr: u32,
        cl_value_size: u32,
    ) -> Result<CLValue, ExecError> {
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
    ) -> Result<Vec<u8>, ExecError> {
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
            None => {
                return Ok(Err(ApiError::MissingKey));
            }
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
            return Err(ExecError::Interpreter(error.into()).into());
        }

        // SAFETY: For all practical purposes following conversion is assumed to be safe
        let bytes_size: u32 = key_bytes
            .len()
            .try_into()
            .expect("Keys should not serialize to many bytes");
        let size_bytes = bytes_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self.try_get_memory()?.set(bytes_written_ptr, &size_bytes) {
            return Err(ExecError::Interpreter(error.into()).into());
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

        if let Some(payment_purse) = self.context.maybe_payment_purse() {
            if Key::URef(payment_purse).normalize() == key.normalize() {
                warn!("attempt to put_key payment purse");
                return Err(Into::into(ExecError::Revert(ApiError::HandlePayment(
                    handle_payment::Error::AttemptToPersistPaymentPurse as u8,
                ))));
            }
        }
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
        let purse_bytes = purse.into_bytes().map_err(ExecError::BytesRepr)?;
        self.try_get_memory()?
            .set(dest_ptr, &purse_bytes)
            .map_err(|e| ExecError::Interpreter(e.into()).into())
    }

    /// Writes caller (deploy) account public key to dest_ptr in the Wasm
    /// memory.
    fn get_caller(&mut self, output_size: u32) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }
        let value = CLValue::from_t(self.context.get_initiator()).map_err(ExecError::CLValue)?;
        let value_size = value.inner_bytes().len();

        // Save serialized public key into host buffer
        if let Err(error) = self.write_host_buffer(value) {
            return Ok(Err(error));
        }

        // Write output
        let output_size_bytes = value_size.to_le_bytes(); // Wasm is little-endian
        if let Err(error) = self.try_get_memory()?.set(output_size, &output_size_bytes) {
            return Err(ExecError::Interpreter(error.into()).into());
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
        if self.context.get_initiator() == PublicKey::System.to_account_hash() {
            return true;
        }

        if let Some(Caller::Initiator { account_hash }) = self.get_immediate_caller() {
            return account_hash == provided_account_hash;
        }
        false
    }

    /// Writes runtime context's phase to dest_ptr in the Wasm memory.
    fn get_phase(&mut self, dest_ptr: u32) -> Result<(), Trap> {
        let phase = self.context.phase();
        let bytes = phase.into_bytes().map_err(ExecError::BytesRepr)?;
        self.try_get_memory()?
            .set(dest_ptr, &bytes)
            .map_err(|e| ExecError::Interpreter(e.into()).into())
    }

    /// Writes requested field from runtime context's block info to dest_ptr in the Wasm memory.
    fn get_block_info(&self, field_idx: u8, dest_ptr: u32) -> Result<(), Trap> {
        if field_idx == 0 {
            // original functionality
            return self.get_blocktime(dest_ptr);
        }
        let block_info = self.context.get_block_info();

        let mut data: Vec<u8> = vec![];
        if field_idx == 1 {
            data = block_info
                .block_height()
                .into_bytes()
                .map_err(ExecError::BytesRepr)?;
        }
        if field_idx == 2 {
            data = block_info
                .parent_block_hash()
                .into_bytes()
                .map_err(ExecError::BytesRepr)?;
        }
        if field_idx == 3 {
            data = block_info
                .state_hash()
                .into_bytes()
                .map_err(ExecError::BytesRepr)?;
        }
        if data.is_empty() {
            Err(ExecError::InvalidImputedOperation.into())
        } else {
            Ok(self
                .try_get_memory()?
                .set(dest_ptr, &data)
                .map_err(|e| ExecError::Interpreter(e.into()))?)
        }
    }

    /// Writes current blocktime to dest_ptr in Wasm memory.
    fn get_blocktime(&self, dest_ptr: u32) -> Result<(), Trap> {
        let block_info = self.context.get_block_info();
        let blocktime = block_info
            .block_time()
            .into_bytes()
            .map_err(ExecError::BytesRepr)?;
        self.try_get_memory()?
            .set(dest_ptr, &blocktime)
            .map_err(|e| ExecError::Interpreter(e.into()).into())
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
        let call_stack: Vec<CallStackElement> = match self.try_get_stack() {
            Ok(stack) => {
                let caller = stack.call_stack_elements();
                caller.iter().map_into().collect_vec()
            }
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
            return Err(ExecError::Interpreter(error.into()).into());
        }

        if call_stack_len == 0 {
            return Ok(Ok(()));
        }

        let call_stack_cl_value = CLValue::from_t(call_stack).map_err(ExecError::CLValue)?;

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
            return Err(ExecError::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Returns information about the call stack based on a given action.
    fn load_caller_information(
        &mut self,
        information: u8,
        // (Output) Pointer to number of elements in the call stack.
        call_stack_len_ptr: u32,
        // (Output) Pointer to size in bytes of the serialized call stack.
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        if !self.can_write_to_host_buffer() {
            // Exit early if the host buffer is already occupied
            return Ok(Err(ApiError::HostBufferFull));
        }

        let caller_info = match CallerInformation::try_from(information) {
            Ok(info) => info,
            Err(error) => return Ok(Err(error)),
        };

        let caller = match caller_info {
            CallerInformation::Initiator => {
                let initiator_account_hash = self.context.get_initiator();
                let caller = Caller::initiator(initiator_account_hash);
                match CallerInfo::try_from(caller) {
                    Ok(caller_info) => {
                        vec![caller_info]
                    }
                    Err(_) => return Ok(Err(ApiError::CLTypeMismatch)),
                }
            }
            CallerInformation::Immediate => match self.get_immediate_caller() {
                Some(frame) => match CallerInfo::try_from(*frame) {
                    Ok(immediate_info) => {
                        vec![immediate_info]
                    }
                    Err(_) => return Ok(Err(ApiError::CLTypeMismatch)),
                },
                None => return Ok(Err(ApiError::Unhandled)),
            },
            CallerInformation::FullCallChain => match self.try_get_stack() {
                Ok(call_stack) => {
                    let call_stack = call_stack.call_stack_elements().clone();

                    let mut ret = vec![];
                    for caller in call_stack {
                        match CallerInfo::try_from(caller) {
                            Ok(info) => ret.push(info),
                            Err(_) => return Ok(Err(ApiError::CLTypeMismatch)),
                        }
                    }
                    ret
                }
                Err(_) => return Ok(Err(ApiError::Unhandled)),
            },
        };

        let call_stack_len: u32 = match caller.len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };
        let call_stack_len_bytes = call_stack_len.to_le_bytes();

        if let Err(error) = self
            .try_get_memory()?
            .set(call_stack_len_ptr, &call_stack_len_bytes)
        {
            return Err(ExecError::Interpreter(error.into()).into());
        }

        if call_stack_len == 0 {
            return Ok(Ok(()));
        }

        let call_stack_cl_value = CLValue::from_t(caller).map_err(ExecError::CLValue)?;

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
            return Err(ExecError::Interpreter(error.into()).into());
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
                        ExecError::Ret(urefs).into()
                    }
                    Err(e) => e.into(),
                }
            }
            Err(e) => e.into(),
        }
    }

    /// Checks if a [`Key`] is a system contract.
    fn is_system_contract(&self, hash_addr: HashAddr) -> Result<bool, ExecError> {
        self.context.is_system_addressable_entity(&hash_addr)
    }

    fn get_named_argument<T: FromBytes + CLTyped>(
        args: &RuntimeArgs,
        name: &str,
    ) -> Result<T, ExecError> {
        let arg: CLValue = args
            .get(name)
            .cloned()
            .ok_or(ExecError::Revert(ApiError::MissingArgument))?;
        arg.into_t()
            .map_err(|_| ExecError::Revert(ApiError::InvalidArgument))
    }

    fn try_get_named_argument<T: FromBytes + CLTyped>(
        args: &RuntimeArgs,
        name: &str,
    ) -> Result<Option<T>, ExecError> {
        match args.get(name) {
            Some(arg) => {
                let arg = arg
                    .clone()
                    .into_t()
                    .map_err(|_| ExecError::Revert(ApiError::InvalidArgument))?;
                Ok(Some(arg))
            }
            None => Ok(None),
        }
    }

    fn reverter<T: Into<ApiError>>(error: T) -> ExecError {
        let api_error: ApiError = error.into();
        // NOTE: This is special casing needed to keep the native system contracts propagate
        // GasLimit properly to the user. Once support for wasm system contract will be dropped this
        // won't be necessary anymore.
        match api_error {
            ApiError::Mint(mint_error) if mint_error == mint::Error::GasLimit as u8 => {
                ExecError::GasLimit
            }
            ApiError::AuctionError(auction_error)
                if auction_error == auction::Error::GasLimit as u8 =>
            {
                ExecError::GasLimit
            }
            ApiError::HandlePayment(handle_payment_error)
                if handle_payment_error == handle_payment::Error::GasLimit as u8 =>
            {
                ExecError::GasLimit
            }
            api_error => ExecError::Revert(api_error),
        }
    }

    /// Calls host mint contract.
    fn call_host_mint(
        &mut self,
        entry_point_name: &str,
        runtime_args: &RuntimeArgs,
        access_rights: ContextAccessRights,
        stack: RuntimeStack,
    ) -> Result<CLValue, ExecError> {
        let gas_counter = self.gas_counter();

        let mint_hash = self.context.get_system_contract(MINT)?;
        let mint_addr = EntityAddr::new_system(mint_hash.value());

        let mint_named_keys = self
            .context
            .state()
            .borrow_mut()
            .get_named_keys(mint_addr)?;

        let mut named_keys = mint_named_keys;

        let runtime_context = self.context.new_from_self(
            mint_addr.into(),
            EntryPointType::Called,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut mint_runtime = self.new_with_stack(runtime_context, stack);

        let engine_config = self.context.engine_config();
        let system_config = engine_config.system_config();
        let mint_costs = system_config.mint_costs();

        let result = match entry_point_name {
            // Type: `fn mint(amount: U512) -> Result<URef, ExecError>`
            mint::METHOD_MINT => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.mint)?;

                let amount: U512 = Self::get_named_argument(runtime_args, mint::ARG_AMOUNT)?;
                let result: Result<URef, mint::Error> = mint_runtime.mint(amount);
                if let Err(mint::Error::GasLimit) = result {
                    return Err(ExecError::GasLimit);
                }
                CLValue::from_t(result).map_err(Self::reverter)
            })(),
            mint::METHOD_REDUCE_TOTAL_SUPPLY => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.reduce_total_supply)?;

                let amount: U512 = Self::get_named_argument(runtime_args, mint::ARG_AMOUNT)?;
                let result: Result<(), mint::Error> = mint_runtime.reduce_total_supply(amount);
                CLValue::from_t(result).map_err(Self::reverter)
            })(),
            mint::METHOD_BURN => (|| {
                mint_runtime.charge_system_contract_call(mint_costs.burn)?;

                let purse: URef = Self::get_named_argument(runtime_args, mint::ARG_PURSE)?;
                let amount: U512 = Self::get_named_argument(runtime_args, mint::ARG_AMOUNT)?;
                let result: Result<(), mint::Error> = mint_runtime.burn(purse, amount);
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
            // U512, id: Option<u64>) -> Result<(), ExecError>`
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
            // Type: `fn read_base_round_reward() -> Result<U512, ExecError>`
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
            _ => {
                // Code should never reach this point as existence of the entrypoint is validated
                // before reaching this pointt.
                Ok(CLValue::unit())
            }
        };

        // Charge just for the amount that particular entry point cost - using gas cost from the
        // isolated runtime might have a recursive costs whenever system contract calls other system
        // contract.
        self.gas(
            mint_runtime
                .gas_counter()
                .checked_sub(gas_counter)
                .unwrap_or(gas_counter),
        )?;

        // Result still contains a result, but the entrypoints logic does not exit early on errors.
        let ret = result?;

        // Update outer spending approved limit.
        self.context
            .set_remaining_spending_limit(mint_runtime.context.remaining_spending_limit());

        let urefs = utils::extract_urefs(&ret)?;
        self.context.access_rights_extend(&urefs);
        {
            let transfers = self.context.transfers_mut();
            mint_runtime.context.transfers().clone_into(transfers);
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
    ) -> Result<CLValue, ExecError> {
        let gas_counter = self.gas_counter();

        let handle_payment_hash = self.context.get_system_contract(HANDLE_PAYMENT)?;
        let handle_payment_key = if self.context.engine_config().enable_entity {
            Key::AddressableEntity(EntityAddr::System(handle_payment_hash.value()))
        } else {
            Key::Hash(handle_payment_hash.value())
        };

        let handle_payment_named_keys = self
            .context
            .state()
            .borrow_mut()
            .get_named_keys(EntityAddr::System(handle_payment_hash.value()))?;

        let mut named_keys = handle_payment_named_keys;

        let runtime_context = self.context.new_from_self(
            handle_payment_key,
            EntryPointType::Called,
            &mut named_keys,
            access_rights,
            runtime_args.to_owned(),
        );

        let mut runtime = self.new_with_stack(runtime_context, stack);

        let engine_config = self.context.engine_config();
        let system_config = engine_config.system_config();
        let handle_payment_costs = system_config.handle_payment_costs();

        let result = match entry_point_name {
            handle_payment::METHOD_GET_PAYMENT_PURSE => {
                runtime.charge_system_contract_call(handle_payment_costs.get_payment_purse)?;
                match self.context.maybe_payment_purse() {
                    Some(payment_purse) => CLValue::from_t(payment_purse).map_err(Self::reverter),
                    None => {
                        let payment_purse = runtime.get_payment_purse().map_err(Self::reverter)?;
                        self.context.set_payment_purse(payment_purse);
                        CLValue::from_t(payment_purse).map_err(Self::reverter)
                    }
                }
            }
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
            _ => {
                // Code should never reach here as existence of the entrypoint is validated before
                // reaching this point.
                Ok(CLValue::unit())
            }
        };

        self.gas(
            runtime
                .gas_counter()
                .checked_sub(gas_counter)
                .unwrap_or(gas_counter),
        )?;

        let ret = result?;

        let urefs = utils::extract_urefs(&ret)?;
        self.context.access_rights_extend(&urefs);
        {
            let transfers = self.context.transfers_mut();
            runtime.context.transfers().clone_into(transfers);
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
    ) -> Result<CLValue, ExecError> {
        let gas_counter = self.gas_counter();

        let entity_hash = self.context.get_system_contract(AUCTION)?;
        let auction_key = Key::addressable_entity_key(EntityKindTag::System, entity_hash);

        let auction_named_keys = self
            .context
            .state()
            .borrow_mut()
            .get_named_keys(EntityAddr::System(entity_hash.value()))?;

        let mut named_keys = auction_named_keys;

        let runtime_context = self.context.new_from_self(
            auction_key,
            EntryPointType::Called,
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

                let global_minimum_delegation_amount =
                    self.context.engine_config().minimum_delegation_amount();
                let minimum_delegation_amount = Self::try_get_named_argument(
                    runtime_args,
                    auction::ARG_MINIMUM_DELEGATION_AMOUNT,
                )?
                .unwrap_or(global_minimum_delegation_amount);

                let global_maximum_delegation_amount =
                    self.context.engine_config().maximum_delegation_amount();
                let maximum_delegation_amount = Self::try_get_named_argument(
                    runtime_args,
                    auction::ARG_MAXIMUM_DELEGATION_AMOUNT,
                )?
                .unwrap_or(global_maximum_delegation_amount);

                if minimum_delegation_amount < global_minimum_delegation_amount
                    || maximum_delegation_amount > global_maximum_delegation_amount
                    || minimum_delegation_amount > maximum_delegation_amount
                {
                    return Err(ExecError::Revert(ApiError::InvalidDelegationAmountLimits));
                }
                let reserved_slots =
                    Self::try_get_named_argument(runtime_args, auction::ARG_RESERVED_SLOTS)?
                        .unwrap_or(0);

                let max_delegators_per_validator =
                    self.context.engine_config().max_delegators_per_validator();

                let minimum_bid_amount = self.context().engine_config().minimum_bid_amount();

                let result = runtime
                    .add_bid(
                        account_hash,
                        delegation_rate,
                        amount,
                        minimum_delegation_amount,
                        maximum_delegation_amount,
                        minimum_bid_amount,
                        max_delegators_per_validator,
                        reserved_slots,
                    )
                    .map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_WITHDRAW_BID => (|| {
                runtime.charge_system_contract_call(auction_costs.withdraw_bid)?;

                let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
                let min_bid_amount = self.context.engine_config().minimum_bid_amount();

                let result = runtime
                    .withdraw_bid(public_key, amount, min_bid_amount)
                    .map_err(Self::reverter)?;
                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_DELEGATE => (|| {
                runtime.charge_system_contract_call(auction_costs.delegate)?;

                let delegator = {
                    match Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR) {
                        Ok(pk) => DelegatorKind::PublicKey(pk),
                        Err(_) => {
                            let uref: URef = match Self::get_named_argument(
                                runtime_args,
                                auction::ARG_DELEGATOR_PURSE,
                            ) {
                                Ok(uref) => uref,
                                Err(err) => {
                                    debug!(%err, "failed to get delegator purse argument");
                                    return Err(err);
                                }
                            };
                            DelegatorKind::Purse(uref.addr())
                        }
                    }
                };
                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

                let max_delegators_per_validator =
                    self.context.engine_config().max_delegators_per_validator();

                let result = runtime
                    .delegate(delegator, validator, amount, max_delegators_per_validator)
                    .map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_UNDELEGATE => (|| {
                runtime.charge_system_contract_call(auction_costs.undelegate)?;

                let delegator = {
                    match Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR) {
                        Ok(pk) => DelegatorKind::PublicKey(pk),
                        Err(_) => {
                            let uref: URef = match Self::get_named_argument(
                                runtime_args,
                                auction::ARG_DELEGATOR_PURSE,
                            ) {
                                Ok(uref) => uref,
                                Err(err) => {
                                    debug!(%err, "failed to get delegator purse argument");
                                    return Err(err);
                                }
                            };
                            DelegatorKind::Purse(uref.addr())
                        }
                    }
                };
                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;

                let result = runtime
                    .undelegate(delegator, validator, amount)
                    .map_err(Self::reverter)?;

                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_REDELEGATE => (|| {
                runtime.charge_system_contract_call(auction_costs.redelegate)?;

                let delegator = {
                    match Self::get_named_argument(runtime_args, auction::ARG_DELEGATOR) {
                        Ok(pk) => DelegatorKind::PublicKey(pk),
                        Err(_) => {
                            let uref: URef = match Self::get_named_argument(
                                runtime_args,
                                auction::ARG_DELEGATOR_PURSE,
                            ) {
                                Ok(uref) => uref,
                                Err(err) => {
                                    debug!(%err, "failed to get delegator purse argument");
                                    return Err(err);
                                }
                            };
                            DelegatorKind::Purse(uref.addr())
                        }
                    }
                };
                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let amount = Self::get_named_argument(runtime_args, auction::ARG_AMOUNT)?;
                let new_validator =
                    Self::get_named_argument(runtime_args, auction::ARG_NEW_VALIDATOR)?;

                let result = runtime
                    .redelegate(delegator, validator, amount, new_validator)
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
                runtime
                    .run_auction(
                        era_end_timestamp_millis,
                        evicted_validators,
                        max_delegators_per_validator,
                        true,
                        Ratio::new_raw(U512::from(1), U512::from(5)),
                    )
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn slash(validator_account_hashes: &[AccountHash]) -> Result<(), ExecError>`
            auction::METHOD_SLASH => (|| {
                runtime.context.charge_gas(auction_costs.slash.into())?;

                let validator_public_keys =
                    Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR_PUBLIC_KEYS)?;
                runtime
                    .slash(validator_public_keys)
                    .map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn distribute(reward_factors: BTreeMap<PublicKey, u64>) -> Result<(),
            // ExecError>`
            auction::METHOD_DISTRIBUTE => (|| {
                runtime
                    .context
                    .charge_gas(auction_costs.distribute.into())?;
                let rewards = Self::get_named_argument(runtime_args, auction::ARG_REWARDS_MAP)?;
                runtime.distribute(rewards).map_err(Self::reverter)?;
                CLValue::from_t(()).map_err(Self::reverter)
            })(),

            // Type: `fn read_era_id() -> Result<EraId, ExecError>`
            auction::METHOD_READ_ERA_ID => (|| {
                runtime
                    .context
                    .charge_gas(auction_costs.read_era_id.into())?;

                let result = runtime.read_era_id().map_err(Self::reverter)?;
                CLValue::from_t(result).map_err(Self::reverter)
            })(),

            auction::METHOD_ACTIVATE_BID => (|| {
                runtime.charge_system_contract_call(auction_costs.activate_bid)?;

                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;

                runtime.activate_bid(validator).map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),
            auction::METHOD_CHANGE_BID_PUBLIC_KEY => (|| {
                runtime.charge_system_contract_call(auction_costs.change_bid_public_key)?;

                let public_key = Self::get_named_argument(runtime_args, auction::ARG_PUBLIC_KEY)?;
                let new_public_key =
                    Self::get_named_argument(runtime_args, auction::ARG_NEW_PUBLIC_KEY)?;

                runtime
                    .change_bid_public_key(public_key, new_public_key)
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),
            auction::METHOD_ADD_RESERVATIONS => (|| {
                runtime.charge_system_contract_call(auction_costs.add_reservations)?;

                let reservations =
                    Self::get_named_argument(runtime_args, auction::ARG_RESERVATIONS)?;

                runtime
                    .add_reservations(reservations)
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),
            auction::METHOD_CANCEL_RESERVATIONS => (|| {
                runtime.charge_system_contract_call(auction_costs.cancel_reservations)?;

                let validator = Self::get_named_argument(runtime_args, auction::ARG_VALIDATOR)?;
                let delegators = Self::get_named_argument(runtime_args, auction::ARG_DELEGATORS)?;
                let max_delegators_per_validator =
                    self.context.engine_config().max_delegators_per_validator();

                runtime
                    .cancel_reservations(validator, delegators, max_delegators_per_validator)
                    .map_err(Self::reverter)?;

                CLValue::from_t(()).map_err(Self::reverter)
            })(),
            _ => {
                // Code should never reach here as existence of the entrypoint is validated before
                // reaching this point.
                Ok(CLValue::unit())
            }
        };

        // Charge for the gas spent during execution in an isolated runtime.
        self.gas(
            runtime
                .gas_counter()
                .checked_sub(gas_counter)
                .unwrap_or(gas_counter),
        )?;

        // Result still contains a result, but the entrypoints logic does not exit early on errors.
        let ret = result?;

        let urefs = utils::extract_urefs(&ret)?;
        self.context.access_rights_extend(&urefs);
        {
            let transfers = self.context.transfers_mut();
            runtime.context.transfers().clone_into(transfers);
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
    ) -> Result<CLValue, ExecError> {
        self.stack = Some(stack);

        self.call_contract(contract_hash, entry_point_name, args)
    }

    pub(crate) fn execute_module_bytes(
        &mut self,
        module_bytes: &Bytes,
        stack: RuntimeStack,
    ) -> Result<CLValue, ExecError> {
        let protocol_version = self.context.protocol_version();
        let engine_config = self.context.engine_config();
        let wasm_config = engine_config.wasm_config();
        #[cfg(feature = "test-support")]
        let max_stack_height = wasm_config.v1().max_stack_height();
        let module = preprocess(*wasm_config, module_bytes)?;
        let (instance, memory) =
            utils::instance_and_memory(module.clone(), protocol_version, engine_config)?;
        self.memory = Some(memory);
        self.module = Some(module);
        self.stack = Some(stack);
        self.context.set_args(utils::attenuate_uref_in_args(
            self.context.args().clone(),
            self.context
                .runtime_footprint()
                .borrow()
                .main_purse()
                .expect("line 1183")
                .addr(),
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
            let downcasted_error = host_error.downcast_ref::<ExecError>();
            return match downcasted_error {
                Some(ExecError::Ret(ref _ret_urefs)) => self
                    .take_host_buffer()
                    .ok_or(ExecError::ExpectedReturnValue),
                Some(error) => Err(error.clone()),
                None => Err(ExecError::Interpreter(host_error.to_string())),
            };
        }
        Err(ExecError::Interpreter(error.into()))
    }

    /// Calls contract living under a `key`, with supplied `args`.
    pub fn call_contract(
        &mut self,
        contract_hash: AddressableEntityHash,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Result<CLValue, ExecError> {
        let contract_hash = contract_hash.value();
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
    ) -> Result<CLValue, ExecError> {
        let contract_package_hash = contract_package_hash.value();
        let identifier = CallContractIdentifier::ContractPackage {
            contract_package_hash,
            version: contract_version,
        };

        self.execute_contract(identifier, &entry_point_name, args)
    }

    fn get_key_from_entity_addr(&self, entity_addr: EntityAddr) -> Key {
        if self.context().engine_config().enable_entity {
            Key::AddressableEntity(entity_addr)
        } else {
            match entity_addr {
                EntityAddr::System(system_hash_addr) => Key::Hash(system_hash_addr),
                EntityAddr::Account(account_hash) => Key::Account(AccountHash::new(account_hash)),
                EntityAddr::SmartContract(contract_hash_addr) => Key::Hash(contract_hash_addr),
            }
        }
    }

    fn get_context_key_for_contract_call(
        &self,
        entity_addr: EntityAddr,
        entry_point: &EntryPoint,
    ) -> Result<Key, ExecError> {
        let current = self.context.entry_point_type();
        let next = entry_point.entry_point_type();
        match (current, next) {
            (EntryPointType::Called, EntryPointType::Caller) => {
                // Session code can't be called from Contract code for security reasons.
                Err(ExecError::InvalidContext)
            }
            (EntryPointType::Factory, EntryPointType::Caller) => {
                // Session code can't be called from Installer code for security reasons.
                Err(ExecError::InvalidContext)
            }
            (EntryPointType::Caller, EntryPointType::Caller) => {
                // Session code called from session reuses current base key
                Ok(self.context.get_context_key())
            }
            (EntryPointType::Caller, EntryPointType::Called)
            | (EntryPointType::Called, EntryPointType::Called) => {
                Ok(self.get_key_from_entity_addr(entity_addr))
            }
            _ => {
                // Any other combination (installer, normal, etc.) is a contract context.
                Ok(self.get_key_from_entity_addr(entity_addr))
            }
        }
    }

    fn try_get_memory(&self) -> Result<&MemoryRef, ExecError> {
        self.memory.as_ref().ok_or(ExecError::WasmPreprocessing(
            PreprocessingError::MissingMemorySection,
        ))
    }

    fn try_get_module(&self) -> Result<&Module, ExecError> {
        self.module.as_ref().ok_or(ExecError::WasmPreprocessing(
            PreprocessingError::MissingModule,
        ))
    }

    fn try_get_stack(&self) -> Result<&RuntimeStack, ExecError> {
        self.stack.as_ref().ok_or(ExecError::MissingRuntimeStack)
    }

    fn maybe_system_type(&self, hash_addr: HashAddr) -> Option<SystemEntityType> {
        let is_mint = self.is_mint(hash_addr);
        if is_mint.is_some() {
            return is_mint;
        };

        let is_auction = self.is_auction(hash_addr);
        if is_auction.is_some() {
            return is_auction;
        };
        let is_handle = self.is_handle_payment(hash_addr);
        if is_handle.is_some() {
            return is_handle;
        };

        None
    }

    fn is_mint(&self, hash_addr: HashAddr) -> Option<SystemEntityType> {
        let hash = match self.context.get_system_contract(MINT) {
            Ok(hash) => hash,
            Err(_) => {
                error!("Failed to get system mint contract hash");
                return None;
            }
        };
        if hash.value() == hash_addr {
            Some(SystemEntityType::Mint)
        } else {
            None
        }
    }

    /// Checks if current context is the `handle_payment` system contract.
    fn is_handle_payment(&self, hash_addr: HashAddr) -> Option<SystemEntityType> {
        let hash = match self.context.get_system_contract(HANDLE_PAYMENT) {
            Ok(hash) => hash,
            Err(_) => {
                error!("Failed to get system handle payment contract hash");
                return None;
            }
        };
        if hash.value() == hash_addr {
            Some(SystemEntityType::HandlePayment)
        } else {
            None
        }
    }

    /// Checks if given hash is the auction system contract.
    fn is_auction(&self, hash_addr: HashAddr) -> Option<SystemEntityType> {
        let hash = match self.context.get_system_contract(AUCTION) {
            Ok(hash) => hash,
            Err(_) => {
                error!("Failed to get system auction contract hash");
                return None;
            }
        };

        if hash.value() == hash_addr {
            Some(SystemEntityType::Auction)
        } else {
            None
        }
    }

    fn execute_contract(
        &mut self,
        identifier: CallContractIdentifier,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Result<CLValue, ExecError> {
        let (footprint, entity_addr, package) = match identifier {
            CallContractIdentifier::Contract { contract_hash } => {
                let entity_addr = if self.context.is_system_addressable_entity(&contract_hash)? {
                    EntityAddr::new_system(contract_hash)
                } else {
                    EntityAddr::new_smart_contract(contract_hash)
                };
                let footprint = match self.context.read_gs(&Key::Hash(contract_hash))? {
                    Some(StoredValue::Contract(contract)) => {
                        if self.context.engine_config().enable_entity {
                            self.migrate_contract_and_contract_package(contract_hash)?;
                        };

                        let maybe_system_entity_type = self.maybe_system_type(contract_hash);

                        RuntimeFootprint::new_contract_footprint(
                            ContractHash::new(contract_hash),
                            contract,
                            maybe_system_entity_type,
                        )
                    }
                    Some(_) | None => {
                        if !self.context.engine_config().enable_entity {
                            return Err(ExecError::KeyNotFound(Key::Hash(contract_hash)));
                        }
                        let key = Key::AddressableEntity(entity_addr);
                        let entity = self.context.read_gs_typed::<AddressableEntity>(&key)?;
                        let entity_named_keys = self
                            .context
                            .state()
                            .borrow_mut()
                            .get_named_keys(entity_addr)?;
                        let entry_points = self.context.get_casper_vm_v1_entry_point(key)?;
                        RuntimeFootprint::new_entity_footprint(
                            entity_addr,
                            entity,
                            entity_named_keys,
                            entry_points,
                        )
                    }
                };

                let package_hash = footprint
                    .package_hash()
                    .ok_or_else(|| ExecError::InvalidContext)?;
                let package: Package = self.context.get_package(package_hash)?;

                // System contract hashes are disabled at upgrade point
                let is_calling_system_contract = self.is_system_contract(contract_hash)?;

                let entity_hash = AddressableEntityHash::new(contract_hash);

                // Check if provided contract hash is disabled
                let is_contract_enabled = package.is_entity_enabled(&entity_hash);

                if !is_calling_system_contract && !is_contract_enabled {
                    return Err(ExecError::DisabledEntity(entity_hash));
                }

                (footprint, entity_addr, package)
            }
            CallContractIdentifier::ContractPackage {
                contract_package_hash,
                version,
            } => {
                let package = self.context.get_package(contract_package_hash)?;

                let entity_version_key = match version {
                    Some(version) => EntityVersionKey::new(
                        self.context.protocol_version().value().major,
                        version,
                    ),
                    None => match package.current_entity_version() {
                        Some(v) => v,
                        None => {
                            return Err(ExecError::NoActiveEntityVersions(
                                contract_package_hash.into(),
                            ));
                        }
                    },
                };

                if package.is_version_missing(entity_version_key) {
                    return Err(ExecError::MissingEntityVersion(entity_version_key));
                }

                if !package.is_version_enabled(entity_version_key) {
                    return Err(ExecError::DisabledEntityVersion(entity_version_key));
                }

                let hash_addr = package
                    .lookup_entity_hash(entity_version_key)
                    .copied()
                    .ok_or(ExecError::MissingEntityVersion(entity_version_key))?
                    .value();

                let entity_addr = if self.context.is_system_addressable_entity(&hash_addr)? {
                    EntityAddr::new_system(hash_addr)
                } else {
                    EntityAddr::new_smart_contract(hash_addr)
                };

                let footprint = match self.context.read_gs(&Key::Hash(hash_addr))? {
                    Some(StoredValue::Contract(contract)) => {
                        if self.context.engine_config().enable_entity {
                            self.migrate_contract_and_contract_package(hash_addr)?;
                        };
                        let maybe_system_entity_type = self.maybe_system_type(hash_addr);
                        RuntimeFootprint::new_contract_footprint(
                            ContractHash::new(hash_addr),
                            contract,
                            maybe_system_entity_type,
                        )
                    }
                    Some(_) | None => {
                        if !self.context.engine_config().enable_entity {
                            return Err(ExecError::KeyNotFound(Key::Hash(hash_addr)));
                        }
                        let key = Key::AddressableEntity(entity_addr);
                        let entity = self.context.read_gs_typed::<AddressableEntity>(&key)?;
                        let entity_named_keys = self
                            .context
                            .state()
                            .borrow_mut()
                            .get_named_keys(entity_addr)?;
                        let entry_points = self.context.get_casper_vm_v1_entry_point(key)?;
                        RuntimeFootprint::new_entity_footprint(
                            entity_addr,
                            entity,
                            entity_named_keys,
                            entry_points,
                        )
                    }
                };

                (footprint, entity_addr, package)
            }
        };

        if let EntityKind::Account(_) = footprint.entity_kind() {
            return Err(ExecError::InvalidContext);
        }

        let entry_point = match footprint.entry_points().get(entry_point_name) {
            Some(entry_point) => entry_point,
            None => {
                match footprint.entity_kind() {
                    EntityKind::System(_) => {
                        self.charge_system_contract_call(
                            self.context()
                                .engine_config()
                                .system_config()
                                .no_such_entrypoint(),
                        )?;
                    }
                    EntityKind::Account(_) => {}
                    EntityKind::SmartContract(_) => {}
                }
                return Err(ExecError::NoSuchMethod(entry_point_name.to_owned()));
            }
        };

        let entry_point_type = entry_point.entry_point_type();

        if self.context.engine_config().enable_entity && entry_point_type.is_invalid_context() {
            return Err(ExecError::InvalidContext);
        }

        // Get contract entry point hash
        // if public, allowed
        // if group, restricted to user group access
        // if template, not allowed
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
                        return Err(ExecError::type_mismatch(
                            param.cl_type().clone(),
                            named_arg.cl_value().cl_type().clone(),
                        ));
                    }
                } else if !param.cl_type().is_option() {
                    return Err(ExecError::MissingArgument {
                        name: param.name().to_string(),
                    });
                }
            }
        }

        let entity_hash = AddressableEntityHash::new(entity_addr.value());

        if !self
            .context
            .engine_config()
            .administrative_accounts()
            .is_empty()
            && !package.is_entity_enabled(&entity_hash)
            && !self
                .context
                .is_system_addressable_entity(&entity_addr.value())?
        {
            return Err(ExecError::DisabledEntity(entity_hash));
        }

        // if session the caller's context
        // else the called contract's context
        let context_entity_key =
            self.get_context_key_for_contract_call(entity_addr, entry_point)?;

        let context_entity_hash = context_entity_key
            .into_entity_hash_addr()
            .ok_or_else(|| ExecError::UnexpectedKeyVariant(context_entity_key))?;

        let (should_attenuate_urefs, should_validate_urefs) = {
            // Determines if this call originated from the system account based on a first
            // element of the call stack.
            let is_system_account =
                self.context.get_initiator() == PublicKey::System.to_account_hash();
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
                self.context
                    .runtime_footprint()
                    .borrow()
                    .main_purse()
                    .expect("need purse for attenuation")
                    .addr(),
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

        let (mut named_keys, access_rights) = match entry_point_type {
            EntryPointType::Caller => {
                let mut access_rights = self
                    .context
                    .runtime_footprint()
                    .borrow()
                    .extract_access_rights(context_entity_hash);
                access_rights.extend(&extended_access_rights);

                let named_keys = self
                    .context
                    .runtime_footprint()
                    .borrow()
                    .named_keys()
                    .clone();

                (named_keys, access_rights)
            }
            EntryPointType::Called | EntryPointType::Factory => {
                let mut access_rights = footprint.extract_access_rights(entity_hash.value());
                access_rights.extend(&extended_access_rights);
                let named_keys = footprint.named_keys().clone();
                (named_keys, access_rights)
            }
        };

        let stack = {
            let mut stack = self.try_get_stack()?.clone();

            let package_hash = match footprint.package_hash() {
                Some(hash) => PackageHash::new(hash),
                None => {
                    return Err(ExecError::UnexpectedStoredValueVariant);
                }
            };

            let caller = if self.context.engine_config().enable_entity {
                Caller::entity(package_hash, entity_addr)
            } else {
                Caller::smart_contract(
                    ContractPackageHash::new(package_hash.value()),
                    ContractHash::new(entity_addr.value()),
                )
            };

            stack.push(caller)?;

            stack
        };

        if let EntityKind::System(system_contract_type) = footprint.entity_kind() {
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
            let byte_code_addr = footprint
                .wasm_hash()
                .ok_or_else(|| ExecError::InvalidContext)?;

            let byte_code_key = match footprint.entity_kind() {
                EntityKind::System(_) | EntityKind::Account(_) => {
                    Key::ByteCode(ByteCodeAddr::Empty)
                }
                EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1) => {
                    if self.context.engine_config().enable_entity {
                        Key::ByteCode(ByteCodeAddr::new_wasm_addr(byte_code_addr))
                    } else {
                        Key::Hash(byte_code_addr)
                    }
                }
                EntityKind::SmartContract(runtime @ ContractRuntimeTag::VmCasperV2) => {
                    return Err(ExecError::IncompatibleRuntime(runtime));
                }
            };

            let byte_code: ByteCode = match self.context.read_gs(&byte_code_key)? {
                Some(StoredValue::ContractWasm(wasm)) => {
                    ByteCode::new(ByteCodeKind::V1CasperWasm, wasm.take_bytes())
                }
                Some(StoredValue::ByteCode(byte_code)) => byte_code,
                Some(_) => {
                    return Err(ExecError::InvalidByteCode(ByteCodeHash::new(
                        byte_code_addr,
                    )))
                }
                None => return Err(ExecError::KeyNotFound(byte_code_key)),
            };

            casper_wasm::deserialize_buffer(byte_code.bytes())?
        };

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
        runtime.context.transfers().clone_into(transfers);

        return match result {
            Ok(_) => {
                // If `Ok` and the `host_buffer` is `None`, the contract's execution succeeded but
                // did not explicitly call `runtime::ret()`.  Treat as though the
                // execution returned the unit type `()` as per Rust functions which
                // don't specify a return value.
                if self.context.entry_point_type() == EntryPointType::Caller
                    && runtime.context.entry_point_type() == EntryPointType::Caller
                {
                    // Overwrites parent's named keys with child's new named key but only when
                    // running session code.
                    *self.context.named_keys_mut() = runtime.context.named_keys().clone();
                }
                self.context
                    .set_remaining_spending_limit(runtime.context.remaining_spending_limit());
                Ok(runtime.take_host_buffer().unwrap_or(CLValue::from_t(())?))
            }
            Err(error) => {
                #[cfg(feature = "test-support")]
                dump_runtime_stack_info(
                    instance,
                    self.context
                        .engine_config()
                        .wasm_config()
                        .v1()
                        .max_stack_height(),
                );
                if let Some(host_error) = error.as_host_error() {
                    // If the "error" was in fact a trap caused by calling `ret` then this is normal
                    // operation and we should return the value captured in the Runtime result
                    // field.
                    let downcasted_error = host_error.downcast_ref::<ExecError>();
                    return match downcasted_error {
                        Some(ExecError::Ret(ref ret_urefs)) => {
                            // Insert extra urefs returned from call.
                            // Those returned URef's are guaranteed to be valid as they were already
                            // validated in the `ret` call inside context we ret from.
                            self.context.access_rights_extend(ret_urefs);
                            if self.context.entry_point_type() == EntryPointType::Caller
                                && runtime.context.entry_point_type() == EntryPointType::Caller
                            {
                                // Overwrites parent's named keys with child's new named key but
                                // only when running session code.
                                *self.context.named_keys_mut() =
                                    runtime.context.named_keys().clone();
                            }
                            // Stored contracts are expected to always call a `ret` function,
                            // otherwise it's an error.
                            runtime
                                .take_host_buffer()
                                .ok_or(ExecError::ExpectedReturnValue)
                        }
                        Some(error) => Err(error.clone()),
                        None => Err(ExecError::Interpreter(host_error.to_string())),
                    };
                }
                Err(ExecError::Interpreter(error.into()))
            }
        };
    }

    fn call_contract_host_buffer(
        &mut self,
        contract_hash: AddressableEntityHash,
        entry_point_name: &str,
        args_bytes: &[u8],
        result_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, ExecError> {
        // Exit early if the host buffer is already occupied
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        let args: RuntimeArgs = bytesrepr::deserialize_from_slice(args_bytes)?;

        if let Some(payment_purse) = self.context.maybe_payment_purse() {
            for named_arg in args.named_args() {
                if utils::extract_urefs(named_arg.cl_value())?
                    .into_iter()
                    .any(|uref| uref.remove_access_rights() == payment_purse.remove_access_rights())
                {
                    warn!("attempt to call_contract with payment purse");

                    return Err(Into::into(ExecError::Revert(ApiError::HandlePayment(
                        handle_payment::Error::AttemptToPersistPaymentPurse as u8,
                    ))));
                }
            }
        }

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
    ) -> Result<Result<(), ApiError>, ExecError> {
        // Exit early if the host buffer is already occupied
        if let Err(err) = self.check_host_buffer() {
            return Ok(Err(err));
        }
        let args: RuntimeArgs = bytesrepr::deserialize_from_slice(args_bytes)?;

        if let Some(payment_purse) = self.context.maybe_payment_purse() {
            for named_arg in args.named_args() {
                if utils::extract_urefs(named_arg.cl_value())?
                    .into_iter()
                    .any(|uref| uref.remove_access_rights() == payment_purse.remove_access_rights())
                {
                    warn!("attempt to call_versioned_contract with payment purse");

                    return Err(Into::into(ExecError::Revert(ApiError::HandlePayment(
                        handle_payment::Error::AttemptToPersistPaymentPurse as u8,
                    ))));
                }
            }
        }

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
    ) -> Result<Result<(), ApiError>, ExecError> {
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
            return Err(ExecError::Interpreter(error.into()));
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
            return Err(ExecError::Interpreter(error.into()).into());
        }

        if total_keys == 0 {
            // No need to do anything else, we leave host buffer empty.
            return Ok(Ok(()));
        }

        let named_keys =
            CLValue::from_t(self.context.named_keys().clone()).map_err(ExecError::CLValue)?;

        let length: u32 = match named_keys.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::BufferTooSmall)),
        };

        if let Err(error) = self.write_host_buffer(named_keys) {
            return Ok(Err(error));
        }

        let length_bytes = length.to_le_bytes();
        if let Err(error) = self.try_get_memory()?.set(result_size_ptr, &length_bytes) {
            return Err(ExecError::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    fn create_contract_package(
        &mut self,
        is_locked: PackageStatus,
    ) -> Result<(ContractPackage, URef), ExecError> {
        let access_key = self.context.new_unit_uref()?;
        let package_status = match is_locked {
            PackageStatus::Locked => ContractPackageStatus::Locked,
            PackageStatus::Unlocked => ContractPackageStatus::Unlocked,
        };

        let contract_package = ContractPackage::new(
            access_key,
            ContractVersions::default(),
            DisabledVersions::default(),
            Groups::default(),
            package_status,
        );

        Ok((contract_package, access_key))
    }

    fn create_package(&mut self, is_locked: PackageStatus) -> Result<(Package, URef), ExecError> {
        let access_key = self.context.new_unit_uref()?;
        let contract_package = Package::new(
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
    ) -> Result<([u8; 32], [u8; 32]), ExecError> {
        let addr = self.context.new_hash_address()?;
        let access_key = if self.context.engine_config().enable_entity {
            let (package, access_key) = self.create_package(lock_status)?;
            self.context
                .metered_write_gs_unsafe(Key::SmartContract(addr), package)?;
            access_key
        } else {
            let (package, access_key) = self.create_contract_package(lock_status)?;
            self.context
                .metered_write_gs_unsafe(Key::Hash(addr), package)?;
            access_key
        };
        Ok((addr, access_key.addr()))
    }

    fn create_contract_user_group_by_contract_package(
        &mut self,
        contract_package_hash: PackageHash,
        label: String,
        num_new_urefs: u32,
        mut existing_urefs: BTreeSet<URef>,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, ExecError> {
        let mut contract_package: ContractPackage = self
            .context
            .get_validated_contract_package(contract_package_hash.value())?;

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
            return Err(ExecError::Interpreter(error.into()));
        }

        // Write updated package to the global state
        self.context.metered_write_gs_unsafe(
            ContractPackageHash::new(contract_package_hash.value()),
            contract_package,
        )?;

        Ok(Ok(()))
    }

    fn create_contract_user_group(
        &mut self,
        contract_package_hash: PackageHash,
        label: String,
        num_new_urefs: u32,
        mut existing_urefs: BTreeSet<URef>,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, ExecError> {
        if !self.context.engine_config().enable_entity {
            return self.create_contract_user_group_by_contract_package(
                contract_package_hash,
                label,
                num_new_urefs,
                existing_urefs,
                output_size_ptr,
            );
        };

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
            return Err(ExecError::Interpreter(error.into()));
        }

        // Write updated package to the global state
        self.context
            .metered_write_gs_unsafe(contract_package_hash, contract_package)?;

        Ok(Ok(()))
    }

    #[allow(clippy::too_many_arguments)]
    fn add_contract_version(
        &mut self,
        package_hash: PackageHash,
        version_ptr: u32,
        entry_points: EntryPoints,
        named_keys: NamedKeys,
        message_topics: BTreeMap<String, MessageTopicOperation>,
        output_ptr: u32,
    ) -> Result<Result<(), ApiError>, ExecError> {
        if self.context.engine_config().enable_entity {
            self.add_contract_version_by_package(
                package_hash,
                version_ptr,
                entry_points,
                named_keys,
                message_topics,
                output_ptr,
            )
        } else {
            self.add_contract_version_by_contract_package(
                package_hash.value(),
                version_ptr,
                entry_points,
                named_keys,
                message_topics,
                output_ptr,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_contract_version_by_contract_package(
        &mut self,
        contract_package_hash: HashAddr,
        version_ptr: u32,
        entry_points: EntryPoints,
        mut named_keys: NamedKeys,
        message_topics: BTreeMap<String, MessageTopicOperation>,
        output_ptr: u32,
    ) -> Result<Result<(), ApiError>, ExecError> {
        if !self.context.install_upgrade_allowed() {
            // NOTE: This is not a permission check on the caller,
            // it is enforcing the rule that only legacy standard deploys (which are grandfathered)
            // and install / upgrade transactions are allowed to call this method
            return Ok(Err(ApiError::NotAllowedToAddContractVersion));
        }

        // if entry_points.contains_stored_session() {
        //     // As of 2.0 we do not allow stored session logic to be
        //     // installed or upgraded. Pre-existing stored
        //     // session logic is still callable.
        //     return Err(ExecError::InvalidEntryPointType);
        // }

        self.context
            .validate_key(&Key::Hash(contract_package_hash))?;

        let mut contract_package: ContractPackage = self
            .context
            .get_validated_contract_package(contract_package_hash)?;

        let version = contract_package.current_contract_version();

        // Return an error if the contract is locked and has some version associated with it.
        if contract_package.is_locked() && version.is_some() {
            return Err(ExecError::LockedEntity(PackageHash::new(
                contract_package_hash,
            )));
        }

        for (_, key) in named_keys.iter() {
            self.context.validate_key(key)?
        }

        let contract_wasm_hash = self.context.new_hash_address()?;
        let contract_wasm = {
            let module_bytes = self.get_module_from_entry_points(&entry_points)?;
            ContractWasm::new(module_bytes)
        };

        let contract_hash: HashAddr = self.context.new_hash_address()?;

        let protocol_version = self.context.protocol_version();
        let major = protocol_version.value().major;

        let maybe_previous_hash =
            if let Some(previous_contract_hash) = contract_package.current_contract_hash() {
                let previous_contract: Contract =
                    self.context.read_gs_typed(&previous_contract_hash.into())?;

                let previous_named_keys = previous_contract.take_named_keys();
                named_keys.append(previous_named_keys);
                Some(previous_contract_hash.value())
            } else {
                None
            };

        if let Err(err) =
            self.carry_forward_message_topics(maybe_previous_hash, contract_hash, message_topics)?
        {
            return Ok(Err(err));
        };

        let contract_package_hash = ContractPackageHash::new(contract_package_hash);
        let contract = Contract::new(
            contract_package_hash,
            contract_wasm_hash.into(),
            named_keys,
            entry_points.into(),
            protocol_version,
        );

        let insert_contract_result =
            contract_package.insert_contract_version(major, contract_hash.into());

        self.context
            .metered_write_gs_unsafe(Key::Hash(contract_wasm_hash), contract_wasm)?;
        self.context
            .metered_write_gs_unsafe(Key::Hash(contract_hash), contract)?;
        self.context
            .metered_write_gs_unsafe(Key::Hash(contract_package_hash.value()), contract_package)?;

        // set return values to buffer
        {
            let hash_bytes = match contract_hash.to_bytes() {
                Ok(bytes) => bytes,
                Err(error) => return Ok(Err(error.into())),
            };

            // Set serialized hash bytes into the output buffer
            if let Err(error) = self.try_get_memory()?.set(output_ptr, &hash_bytes) {
                return Err(ExecError::Interpreter(error.into()));
            }

            // Set version into VM shared memory
            let version_value: u32 = insert_contract_result.contract_version();
            let version_bytes = version_value.to_le_bytes();
            if let Err(error) = self.try_get_memory()?.set(version_ptr, &version_bytes) {
                return Err(ExecError::Interpreter(error.into()));
            }
        }

        Ok(Ok(()))
    }

    #[allow(clippy::too_many_arguments)]
    fn add_contract_version_by_package(
        &mut self,
        package_hash: PackageHash,
        version_ptr: u32,
        entry_points: EntryPoints,
        mut named_keys: NamedKeys,
        message_topics: BTreeMap<String, MessageTopicOperation>,
        output_ptr: u32,
    ) -> Result<Result<(), ApiError>, ExecError> {
        if !self.context.install_upgrade_allowed() {
            // NOTE: This is not a permission check on the caller,
            // it is enforcing the rule that only legacy standard deploys (which are grandfathered)
            // and install / upgrade transactions are allowed to call this method
            return Ok(Err(ApiError::NotAllowedToAddContractVersion));
        }

        if entry_points.contains_stored_session() {
            // As of 2.0 we do not allow stored session logic to be
            // installed or upgraded. Pre-existing stored
            // session logic is still callable.
            return Err(ExecError::InvalidEntryPointType);
        }

        let mut package = self.context.get_package(package_hash.value())?;

        // Return an error if the contract is locked and has some version associated with it.
        if package.is_locked() {
            return Err(ExecError::LockedEntity(package_hash));
        }

        let (
            main_purse,
            previous_named_keys,
            action_thresholds,
            associated_keys,
            previous_hash_addr,
        ) = self.new_version_entity_parts(&package)?;

        // We generate the byte code hash because a byte code record
        // must exist for a contract record to exist.
        let byte_code_hash = self.context.new_hash_address()?;

        let hash_addr = self.context.new_hash_address()?;

        if let Err(err) =
            self.carry_forward_message_topics(previous_hash_addr, hash_addr, message_topics)?
        {
            return Ok(Err(err));
        };

        let protocol_version = self.context.protocol_version();

        let insert_entity_version_result =
            package.insert_entity_version(protocol_version.value().major, hash_addr.into());

        let byte_code = {
            let module_bytes = self.get_module_from_entry_points(&entry_points)?;
            ByteCode::new(ByteCodeKind::V1CasperWasm, module_bytes)
        };

        self.context.metered_write_gs_unsafe(
            Key::ByteCode(ByteCodeAddr::new_wasm_addr(byte_code_hash)),
            byte_code,
        )?;

        let entity_addr = EntityAddr::new_smart_contract(hash_addr);

        {
            // DO NOT EXTRACT INTO SEPARATE FUNCTION.
            for (_, key) in named_keys.iter() {
                // Validate all the imputed named keys
                // against the installers permissions
                self.context.validate_key(key)?;
            }
            // Carry forward named keys from previous version
            // Grant all the imputed named keys + previous named keys.
            named_keys.append(previous_named_keys);
            for (name, key) in named_keys.iter() {
                let named_key_value =
                    StoredValue::NamedKey(NamedKeyValue::from_concrete_values(*key, name.clone())?);
                let named_key_addr = NamedKeyAddr::new_from_string(entity_addr, name.clone())?;
                self.context
                    .metered_write_gs_unsafe(Key::NamedKey(named_key_addr), named_key_value)?;
            }
            self.context.write_entry_points(entity_addr, entry_points)?;
        }

        let entity = AddressableEntity::new(
            package_hash,
            byte_code_hash.into(),
            protocol_version,
            main_purse,
            associated_keys,
            action_thresholds,
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1),
        );
        let entity_key = Key::AddressableEntity(entity_addr);
        self.context.metered_write_gs_unsafe(entity_key, entity)?;
        self.context
            .metered_write_gs_unsafe(package_hash, package)?;

        // set return values to buffer
        {
            let hash_bytes = match hash_addr.to_bytes() {
                Ok(bytes) => bytes,
                Err(error) => return Ok(Err(error.into())),
            };

            // Set serialized hash bytes into the output buffer
            if let Err(error) = self.try_get_memory()?.set(output_ptr, &hash_bytes) {
                return Err(ExecError::Interpreter(error.into()));
            }

            // Set version into VM shared memory
            let version_value: u32 = insert_entity_version_result.entity_version();
            let version_bytes = version_value.to_le_bytes();
            if let Err(error) = self.try_get_memory()?.set(version_ptr, &version_bytes) {
                return Err(ExecError::Interpreter(error.into()));
            }
        }

        Ok(Ok(()))
    }

    fn carry_forward_message_topics(
        &mut self,
        previous_hash_addr: Option<HashAddr>,
        hash_addr: HashAddr,
        message_topics: BTreeMap<String, MessageTopicOperation>,
    ) -> Result<Result<(), ApiError>, ExecError> {
        let mut previous_message_topics = match previous_hash_addr {
            Some(previous_hash) => self.context.get_message_topics(previous_hash)?,
            None => MessageTopics::default(),
        };

        let max_topics_per_contract = self
            .context
            .engine_config()
            .wasm_config()
            .messages_limits()
            .max_topics_per_contract();

        let topics_to_add = message_topics
            .iter()
            .filter(|(_, operation)| match operation {
                MessageTopicOperation::Add => true,
            });
        // Check if registering the new topics would exceed the limit per contract
        if previous_message_topics.len() + topics_to_add.clone().count()
            > max_topics_per_contract as usize
        {
            return Ok(Err(ApiError::from(MessageTopicError::MaxTopicsExceeded)));
        }

        // Extend the previous topics with the newly added ones.
        for (new_topic, _) in topics_to_add {
            let topic_name_hash = cryptography::blake2b(new_topic.as_bytes()).into();
            if let Err(e) = previous_message_topics.add_topic(new_topic.as_str(), topic_name_hash) {
                return Ok(Err(e.into()));
            }
        }

        for (topic_name, topic_hash) in previous_message_topics.iter() {
            let topic_key = Key::message_topic(hash_addr, *topic_hash);
            let block_time = self.context.get_block_info().block_time();
            let summary = StoredValue::MessageTopic(MessageTopicSummary::new(
                0,
                block_time,
                topic_name.clone(),
            ));
            self.context.metered_write_gs_unsafe(topic_key, summary)?;
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
            Option<HashAddr>,
        ),
        ExecError,
    > {
        if let Some(previous_entity_hash) = package.current_entity_hash() {
            let previous_entity_key = Key::contract_entity_key(previous_entity_hash);
            let (mut previous_entity, requires_purse_creation) =
                self.context.get_contract_entity(previous_entity_key)?;

            let action_thresholds = previous_entity.action_thresholds().clone();

            let associated_keys = previous_entity.associated_keys().clone();
            // STEP 1: LOAD THE CONTRACT AND CHECK IF CALLER IS IN ASSOCIATED KEYS WITH ENOUGH
            // WEIGHT     TO UPGRADE (COMPARE TO THE ACTION THRESHOLD FOR UPGRADE
            // ACTION). STEP 2: IF CALLER IS NOT IN CONTRACTS ASSOCIATED KEYS
            //    CHECK FOR LEGACY UREFADDR UNDER KEY:HASH(PACKAGEADDR)
            //    IF FOUND,
            //      call validate_uref(that uref)
            //    IF VALID,
            //      create the new contract version carrying forward previous state including
            // associated keys      BUT add the caller to the associated keys with
            // weight == to the action threshold for upgrade ELSE, error
            if !previous_entity.can_upgrade_with(self.context.authorization_keys()) {
                // Check if the calling entity must be grandfathered into the new
                // addressable entity format
                let account_hash = self.context.get_initiator();

                let access_key = match self
                    .context
                    .read_gs(&Key::Hash(previous_entity.package_hash().value()))?
                {
                    Some(StoredValue::ContractPackage(contract_package)) => {
                        contract_package.access_key()
                    }
                    Some(StoredValue::CLValue(cl_value)) => {
                        let (_key, uref) = cl_value
                            .into_t::<(Key, URef)>()
                            .map_err(ExecError::CLValue)?;
                        uref
                    }
                    Some(_other) => return Err(ExecError::UnexpectedStoredValueVariant),
                    None => {
                        return Err(ExecError::UpgradeAuthorizationFailure);
                    }
                };

                let has_access = self.context.validate_uref(&access_key).is_ok();

                if has_access && !associated_keys.contains_key(&account_hash) {
                    previous_entity.add_associated_key(
                        account_hash,
                        *action_thresholds.upgrade_management(),
                    )?;
                } else {
                    return Err(ExecError::UpgradeAuthorizationFailure);
                }
            }

            let main_purse = if !requires_purse_creation {
                self.create_purse()?
            } else {
                previous_entity.main_purse()
            };

            let associated_keys = previous_entity.associated_keys().clone();

            let previous_named_keys = self.context.get_named_keys(previous_entity_key)?;

            return Ok((
                main_purse,
                previous_named_keys,
                action_thresholds,
                associated_keys,
                Some(previous_entity_hash.value()),
            ));
        }

        Ok((
            self.create_purse()?,
            NamedKeys::new(),
            ActionThresholds::default(),
            AssociatedKeys::new(self.context.get_initiator(), Weight::new(1)),
            None,
        ))
    }

    fn disable_contract_version(
        &mut self,
        contract_package_hash: PackageHash,
        contract_hash: AddressableEntityHash,
    ) -> Result<Result<(), ApiError>, ExecError> {
        if self.context.engine_config().enable_entity {
            let contract_package_key = Key::SmartContract(contract_package_hash.value());
            self.context.validate_key(&contract_package_key)?;

            let mut contract_package: Package =
                self.context.get_validated_package(contract_package_hash)?;

            if contract_package.is_locked() {
                return Err(ExecError::LockedEntity(contract_package_hash));
            }

            if let Err(err) = contract_package.disable_entity_version(contract_hash) {
                return Ok(Err(err.into()));
            }

            self.context
                .metered_write_gs_unsafe(contract_package_key, contract_package)?;
        } else {
            let contract_package_key = Key::Hash(contract_package_hash.value());
            self.context.validate_key(&contract_package_key)?;

            let mut contract_package: ContractPackage = self
                .context
                .get_validated_contract_package(contract_package_hash.value())?;

            if contract_package.is_locked() {
                return Err(ExecError::LockedEntity(PackageHash::new(
                    contract_package_hash.value(),
                )));
            }
            let contract_hash = ContractHash::new(contract_hash.value());

            if let Err(err) = contract_package.disable_contract_version(contract_hash) {
                return Ok(Err(err.into()));
            }

            self.context
                .metered_write_gs_unsafe(contract_package_key, contract_package)?;
        }

        Ok(Ok(()))
    }

    fn enable_contract_version(
        &mut self,
        contract_package_hash: PackageHash,
        contract_hash: AddressableEntityHash,
    ) -> Result<Result<(), ApiError>, ExecError> {
        if self.context.engine_config().enable_entity {
            let contract_package_key = Key::SmartContract(contract_package_hash.value());
            self.context.validate_key(&contract_package_key)?;

            let mut contract_package: Package =
                self.context.get_validated_package(contract_package_hash)?;

            if contract_package.is_locked() {
                return Err(ExecError::LockedEntity(contract_package_hash));
            }

            if let Err(err) = contract_package.enable_version(contract_hash) {
                return Ok(Err(err.into()));
            }

            self.context
                .metered_write_gs_unsafe(contract_package_key, contract_package)?;
        } else {
            let contract_package_key = Key::Hash(contract_package_hash.value());
            self.context.validate_key(&contract_package_key)?;

            let mut contract_package: ContractPackage = self
                .context
                .get_validated_contract_package(contract_package_hash.value())?;

            if contract_package.is_locked() {
                return Err(ExecError::LockedEntity(PackageHash::new(
                    contract_package_hash.value(),
                )));
            }
            let contract_hash = ContractHash::new(contract_hash.value());

            if let Err(err) = contract_package.enable_contract_version(contract_hash) {
                return Ok(Err(err.into()));
            }

            self.context
                .metered_write_gs_unsafe(contract_package_key, contract_package)?;
        }

        Ok(Ok(()))
    }

    /// Writes function address (`hash_bytes`) into the Wasm memory (at
    /// `dest_ptr` pointer).
    fn function_address(&mut self, hash_bytes: [u8; 32], dest_ptr: u32) -> Result<(), Trap> {
        self.try_get_memory()?
            .set(dest_ptr, &hash_bytes)
            .map_err(|e| ExecError::Interpreter(e.into()).into())
    }

    /// Generates new unforgable reference and adds it to the context's
    /// access_rights set.
    fn new_uref(&mut self, uref_ptr: u32, value_ptr: u32, value_size: u32) -> Result<(), Trap> {
        let cl_value = self.cl_value_from_mem(value_ptr, value_size)?; // read initial value from memory
        let uref = self.context.new_uref(StoredValue::CLValue(cl_value))?;
        self.try_get_memory()?
            .set(uref_ptr, &uref.into_bytes().map_err(ExecError::BytesRepr)?)
            .map_err(|e| ExecError::Interpreter(e.into()).into())
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
    ) -> Result<(), ExecError> {
        if self.context.get_context_key() != self.context.get_system_entity_key(MINT)? {
            return Err(ExecError::InvalidContext);
        }

        if self.context.phase() != Phase::Session {
            return Ok(());
        }

        let txn_hash = self.context.get_transaction_hash();
        let from = InitiatorAddr::AccountHash(self.context.get_initiator());
        let fee = Gas::from(
            self.context
                .engine_config()
                .system_config()
                .mint_costs()
                .transfer,
        );
        let transfer = Transfer::V2(TransferV2::new(
            txn_hash, from, maybe_to, source, target, amount, fee, id,
        ));
        self.context.transfers_mut().push(transfer);
        Ok(())
    }

    /// Records given auction info at a given era id
    fn record_era_info(&mut self, era_info: EraInfo) -> Result<(), ExecError> {
        if self.context.get_initiator() != PublicKey::System.to_account_hash() {
            return Err(ExecError::InvalidContext);
        }

        if self.context.get_context_key() != self.context.get_system_entity_key(AUCTION)? {
            return Err(ExecError::InvalidContext);
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
            Some(stored_value) => {
                CLValue::try_from(stored_value).map_err(ExecError::TypeMismatch)?
            }
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
            return Err(ExecError::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Reverts contract execution with a status specified.
    fn revert(&mut self, status: u32) -> Trap {
        ExecError::Revert(status.into()).into()
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
                .runtime_footprint()
                .borrow()
                .can_manage_keys_with(self.context.authorization_keys());
        }

        if self
            .context
            .engine_config()
            .is_administrator(&self.context.get_initiator())
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
            let source: AccountHash = bytesrepr::deserialize_from_slice(source_serialized)
                .map_err(ExecError::BytesRepr)?;
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
            Err(ExecError::AddKeyFailure(e)) => Ok(e as i32),
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
            let source: AccountHash = bytesrepr::deserialize_from_slice(source_serialized)
                .map_err(ExecError::BytesRepr)?;
            source
        };

        if !self.can_manage_keys() {
            return Ok(RemoveKeyFailure::PermissionDenied as i32);
        }

        match self.context.remove_associated_key(account_hash) {
            Ok(_) => Ok(0),
            Err(ExecError::RemoveKeyFailure(e)) => Ok(e as i32),
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
            let source: AccountHash = bytesrepr::deserialize_from_slice(source_serialized)
                .map_err(ExecError::BytesRepr)?;
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
            Err(ExecError::UpdateKeyFailure(e)) => Ok(e as i32),
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
                    Err(ExecError::SetThresholdFailure(e)) => Ok(e as i32),
                    Err(error) => Err(error.into()),
                }
            }
            Err(_) => Err(Trap::Code(TrapCode::Unreachable)),
        }
    }

    /// Looks up the public mint contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_mint_hash(&self) -> Result<AddressableEntityHash, ExecError> {
        self.context.get_system_contract(MINT)
    }

    /// Looks up the public handle payment contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_handle_payment_hash(&self) -> Result<AddressableEntityHash, ExecError> {
        self.context.get_system_contract(HANDLE_PAYMENT)
    }

    /// Looks up the public standard payment contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_standard_payment_hash(&self) -> Result<AddressableEntityHash, ExecError> {
        self.context.get_system_contract(STANDARD_PAYMENT)
    }

    /// Looks up the public auction contract key in the context's protocol data.
    ///
    /// Returned URef is already attenuated depending on the calling account.
    fn get_auction_hash(&self) -> Result<AddressableEntityHash, ExecError> {
        self.context.get_system_contract(AUCTION)
    }

    /// Calls the `read_base_round_reward` method on the mint contract at the given mint
    /// contract key
    fn mint_read_base_round_reward(
        &mut self,
        mint_contract_hash: AddressableEntityHash,
    ) -> Result<U512, ExecError> {
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
    ) -> Result<URef, ExecError> {
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
    ) -> Result<(), ExecError> {
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
    fn mint_create(
        &mut self,
        mint_contract_hash: AddressableEntityHash,
    ) -> Result<URef, ExecError> {
        let result =
            self.call_contract(mint_contract_hash, mint::METHOD_CREATE, RuntimeArgs::new());
        let purse = result?.into_t()?;
        Ok(purse)
    }

    fn create_purse(&mut self) -> Result<URef, ExecError> {
        let _scoped_host_function_flag = self.host_function_flag.enter_host_function_scope();
        self.mint_create(self.get_mint_hash()?)
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
    ) -> Result<Result<(), mint::Error>, ExecError> {
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
    ) -> Result<TransferResult, ExecError> {
        let mint_contract_hash = self.get_mint_hash()?;

        let allow_unrestricted_transfers =
            self.context.engine_config().allow_unrestricted_transfers();

        if !allow_unrestricted_transfers
            && self.context.get_initiator() != PublicKey::System.to_account_hash()
            && !self
                .context
                .engine_config()
                .is_administrator(&self.context.get_initiator())
            && !self.context.engine_config().is_administrator(&target)
        {
            return Err(ExecError::DisabledUnrestrictedTransfers);
        }

        // A precondition check that verifies that the transfer can be done
        // as the source purse has enough funds to cover the transfer.
        if amount > self.available_balance(source)?.unwrap_or_default() {
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
                let main_purse = target_purse;
                if !self.context.engine_config().enable_entity {
                    let account = Account::create(target, NamedKeys::new(), target_purse);
                    self.context.metered_write_gs_unsafe(
                        Key::Account(target),
                        StoredValue::Account(account),
                    )?;
                    return Ok(Ok(TransferredTo::NewAccount));
                }

                let protocol_version = self.context.protocol_version();
                let byte_code_hash = ByteCodeHash::default();
                let entity_hash = AddressableEntityHash::new(target.value());
                let package_hash = PackageHash::new(self.context.new_hash_address()?);

                let associated_keys = AssociatedKeys::new(target, Weight::new(1));

                let entity = AddressableEntity::new(
                    package_hash,
                    byte_code_hash,
                    protocol_version,
                    main_purse,
                    associated_keys,
                    ActionThresholds::default(),
                    EntityKind::Account(target),
                );

                let package = {
                    let mut package = Package::new(
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

                self.context.metered_write_gs_unsafe(
                    contract_package_key,
                    StoredValue::SmartContract(package),
                )?;

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
    ) -> Result<TransferResult, ExecError> {
        let mint_contract_key = self.get_mint_hash()?;

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
    ) -> Result<TransferResult, ExecError> {
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
    ) -> Result<TransferResult, ExecError> {
        let _scoped_host_function_flag = self.host_function_flag.enter_host_function_scope();
        let target_key = Key::Account(target);

        // Look up the account at the given public key's address
        match self.context.read_gs(&target_key)? {
            None => {
                // If no account exists, create a new account and transfer the amount to its
                // purse.

                self.transfer_to_new_account(source, target, amount, id)
            }
            Some(StoredValue::CLValue(entity_key_value)) => {
                // Attenuate the target main purse
                let entity_key = CLValue::into_t::<Key>(entity_key_value)?;
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
                        return Err(ExecError::UnexpectedKeyVariant(entity_key));
                    };
                    return Err(ExecError::InvalidEntity(contract_hash));
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
                Err(ExecError::AccountNotFound(target_key))
            }
        }
    }

    fn transfer_from_purse_to_account(
        &mut self,
        source: URef,
        target_account: &Account,
        amount: U512,
        id: Option<u64>,
    ) -> Result<TransferResult, ExecError> {
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
    ) -> Result<Result<(), mint::Error>, ExecError> {
        self.context.validate_uref(&source)?;
        let mint_contract_key = self.get_mint_hash()?;
        match self.mint_transfer(mint_contract_key, None, source, target, amount, id)? {
            Ok(()) => Ok(Ok(())),
            Err(mint_error) => Ok(Err(mint_error)),
        }
    }

    fn total_balance(&mut self, purse: URef) -> Result<U512, ExecError> {
        match self.context.total_balance(&purse) {
            Ok(motes) => Ok(motes.value()),
            Err(err) => Err(err),
        }
    }

    fn available_balance(&mut self, purse: URef) -> Result<Option<U512>, ExecError> {
        match self.context.available_balance(&purse) {
            Ok(motes) => Ok(Some(motes.value())),
            Err(err) => Err(err),
        }
    }

    fn get_balance_host_buffer(
        &mut self,
        purse_ptr: u32,
        purse_size: usize,
        output_size_ptr: u32,
    ) -> Result<Result<(), ApiError>, ExecError> {
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

        let balance = match self.available_balance(purse)? {
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
            return Err(ExecError::Interpreter(error.into()));
        }

        Ok(Ok(()))
    }

    fn get_system_contract(
        &mut self,
        system_contract_index: u32,
        dest_ptr: u32,
        _dest_size: u32,
    ) -> Result<Result<(), ApiError>, Trap> {
        let hash: AddressableEntityHash = match SystemEntityType::try_from(system_contract_index) {
            Ok(SystemEntityType::Mint) => self.get_mint_hash()?,
            Ok(SystemEntityType::HandlePayment) => self.get_handle_payment_hash()?,
            Ok(SystemEntityType::StandardPayment) => self.get_standard_payment_hash()?,
            Ok(SystemEntityType::Auction) => self.get_auction_hash()?,
            Err(error) => return Ok(Err(error)),
        };

        match self.try_get_memory()?.set(dest_ptr, hash.as_ref()) {
            Ok(_) => Ok(Ok(())),
            Err(error) => Err(ExecError::Interpreter(error.into()).into()),
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
    ) -> Result<Result<(), ApiError>, ExecError> {
        let (_cl_type, serialized_value) = match self.take_host_buffer() {
            None => return Ok(Err(ApiError::HostBufferEmpty)),
            Some(cl_value) => cl_value.destructure(),
        };

        if serialized_value.len() > u32::MAX as usize {
            return Ok(Err(ApiError::OutOfMemory));
        }
        if serialized_value.len() > dest_size {
            return Ok(Err(ApiError::BufferTooSmall));
        }

        // Slice data, so if `dest_size` is larger than host_buffer size, it will take host_buffer
        // as whole.
        let sliced_buf = &serialized_value[..cmp::min(dest_size, serialized_value.len())];
        if let Err(error) = self.try_get_memory()?.set(dest_ptr, sliced_buf) {
            return Err(ExecError::Interpreter(error.into()));
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
            return Err(ExecError::Interpreter(error.into()));
        }

        Ok(Ok(()))
    }

    #[cfg(feature = "test-support")]
    fn print(&mut self, text_ptr: u32, text_size: u32) -> Result<(), Trap> {
        let text = self.string_from_mem(text_ptr, text_size)?;
        println!("{}", text); // this println! is intentional
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
            Some(arg) if arg.inner_bytes().len() > u32::MAX as usize => {
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
            return Err(ExecError::Interpreter(e.into()).into());
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
            return Err(ExecError::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    /// Enforce group access restrictions (if any) on attempts to call an `EntryPoint`.
    fn validate_entry_point_access(
        &self,
        package: &Package,
        name: &str,
        access: &EntryPointAccess,
    ) -> Result<(), ExecError> {
        match access {
            EntryPointAccess::Public => Ok(()),
            EntryPointAccess::Groups(group_names) => {
                if group_names.is_empty() {
                    // Exits early in a special case of empty list of groups regardless of the group
                    // checking logic below it.
                    return Err(ExecError::InvalidContext);
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
                    return Err(ExecError::InvalidContext);
                }

                Ok(())
            }
            EntryPointAccess::Template => Err(ExecError::TemplateMethod(name.to_string())),
        }
    }

    /// Remove a user group from access to a contract
    fn remove_contract_user_group(
        &mut self,
        package_key: PackageHash,
        label: Group,
    ) -> Result<Result<(), ApiError>, ExecError> {
        if self.context.engine_config().enable_entity {
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
                    self.context
                        .get_casper_vm_v1_entry_point(Key::contract_entity_key(*entity_hash))?
                };
                for entry_point in entry_points.take_entry_points() {
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
        } else {
            let mut contract_package = self
                .context
                .get_validated_contract_package(package_key.value())?;

            let group_to_remove = Group::new(label);
            let groups = contract_package.groups_mut();

            // Ensure group exists in groups
            if !groups.contains(&group_to_remove) {
                return Ok(Err(addressable_entity::Error::GroupDoesNotExist.into()));
            }

            // Remove group if it is not referenced by at least one entry_point in active versions.
            for (_version, contract_hash) in contract_package.versions().iter() {
                let entry_points = {
                    self.context
                        .get_casper_vm_v1_entry_point(Key::contract_entity_key(
                            AddressableEntityHash::new(contract_hash.value()),
                        ))?
                };
                for entry_point in entry_points.take_entry_points() {
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

            if !contract_package.remove_group(&group_to_remove) {
                return Ok(Err(addressable_entity::Error::GroupInUse.into()));
            }

            // Write updated package to the global state
            self.context.metered_write_gs_unsafe(
                ContractPackageHash::new(package_key.value()),
                contract_package,
            )?;
        }
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
    ) -> Result<Result<(), ApiError>, ExecError> {
        let contract_package_hash = self.t_from_mem(package_ptr, package_size)?;
        let label: String = self.t_from_mem(label_ptr, label_size)?;
        let new_uref = if self.context.engine_config().enable_entity {
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

            // Write updated package to the global state
            self.context
                .metered_write_gs_unsafe(contract_package_hash, contract_package)?;
            new_uref
        } else {
            let mut contract_package = self
                .context
                .get_validated_contract_package(contract_package_hash.value())?;
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

            // Write updated package to the global state
            self.context.metered_write_gs_unsafe(
                ContractPackageHash::new(contract_package_hash.value()),
                contract_package,
            )?;
            new_uref
        };

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
            return Err(ExecError::Interpreter(error.into()));
        }

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
    ) -> Result<Result<(), ApiError>, ExecError> {
        let contract_package_hash: PackageHash = self.t_from_mem(package_ptr, package_size)?;
        let label: String = self.t_from_mem(label_ptr, label_size)?;
        let urefs: BTreeSet<URef> = self.t_from_mem(urefs_ptr, urefs_size)?;

        if self.context.engine_config().enable_entity {
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
        } else {
            let contract_package_hash = ContractPackageHash::new(contract_package_hash.value());
            let mut contract_package = self
                .context
                .get_validated_contract_package(contract_package_hash.value())?;

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
        }

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
        let cost = host_function
            .calculate_gas_cost(weights)
            .ok_or(ExecError::GasLimit)?; // Overflowing gas calculation means gas limit was exceeded
        self.gas(cost)?;
        Ok(())
    }

    /// Creates a dictionary
    fn new_dictionary(&mut self, output_size_ptr: u32) -> Result<Result<(), ApiError>, ExecError> {
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
            return Err(ExecError::Interpreter(error.into()));
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
            return Err(ExecError::Interpreter(error.into()).into());
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
            return Err(ExecError::Interpreter(error.into()).into());
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
    fn is_system_immediate_caller(&self) -> Result<bool, ExecError> {
        let immediate_caller = match self.get_immediate_caller() {
            Some(call_stack_element) => call_stack_element,
            None => {
                // Immediate caller is assumed to exist at a time this check is run.
                return Ok(false);
            }
        };

        match immediate_caller {
            Caller::Initiator { account_hash } => {
                // This case can happen during genesis where we're setting up purses for accounts.
                Ok(account_hash == &PublicKey::System.to_account_hash())
            }
            Caller::SmartContract { contract_hash, .. } => Ok(self
                .context
                .is_system_addressable_entity(&contract_hash.value())?),
            Caller::Entity { entity_addr, .. } => Ok(self
                .context
                .is_system_addressable_entity(&entity_addr.value())?),
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
        let authorization_keys = Vec::from_iter(self.context.authorization_keys().clone());

        let total_keys: u32 = match authorization_keys.len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };
        let total_keys_bytes = total_keys.to_le_bytes();
        if let Err(error) = self.try_get_memory()?.set(len_ptr, &total_keys_bytes) {
            return Err(ExecError::Interpreter(error.into()).into());
        }

        if total_keys == 0 {
            // No need to do anything else, we leave host buffer empty.
            return Ok(Ok(()));
        }

        let authorization_keys = CLValue::from_t(authorization_keys).map_err(ExecError::CLValue)?;

        let length: u32 = match authorization_keys.inner_bytes().len().try_into() {
            Ok(value) => value,
            Err(_) => return Ok(Err(ApiError::OutOfMemory)),
        };
        if let Err(error) = self.write_host_buffer(authorization_keys) {
            return Ok(Err(error));
        }

        let length_bytes = length.to_le_bytes();
        if let Err(error) = self.try_get_memory()?.set(result_size_ptr, &length_bytes) {
            return Err(ExecError::Interpreter(error.into()).into());
        }

        Ok(Ok(()))
    }

    fn prune(&mut self, key: Key) {
        self.context.prune_gs_unsafe(key);
    }

    pub(crate) fn migrate_contract_and_contract_package(
        &mut self,
        hash_addr: HashAddr,
    ) -> Result<AddressableEntity, ExecError> {
        let protocol_version = self.context.protocol_version();
        let contract = self.context.get_contract(ContractHash::new(hash_addr))?;
        let package_hash = contract.contract_package_hash();
        self.context
            .migrate_package(package_hash, protocol_version)?;
        let entity_hash = AddressableEntityHash::new(hash_addr);
        let key = Key::contract_entity_key(entity_hash);
        self.context.read_gs_typed(&key)
    }

    fn add_message_topic(&mut self, topic_name: &str) -> Result<Result<(), ApiError>, ExecError> {
        let topic_hash = cryptography::blake2b(topic_name).into();

        self.context
            .add_message_topic(topic_name, topic_hash)
            .map(|ret| ret.map_err(ApiError::from))
    }

    fn emit_message(
        &mut self,
        topic_name: &str,
        message: MessagePayload,
    ) -> Result<Result<(), ApiError>, Trap> {
        let hash_addr = self.context.context_key_to_entity_addr()?.value();

        let topic_name_hash = cryptography::blake2b(topic_name).into();
        let topic_key = Key::Message(MessageAddr::new_topic_addr(hash_addr, topic_name_hash));

        // Check if the topic exists and get the summary.
        let Some(StoredValue::MessageTopic(prev_topic_summary)) =
            self.context.read_gs(&topic_key)?
        else {
            return Ok(Err(ApiError::MessageTopicNotRegistered));
        };

        let current_blocktime = self.context.get_block_info().block_time();
        let topic_message_index = if prev_topic_summary.blocktime() != current_blocktime {
            for index in 1..prev_topic_summary.message_count() {
                self.context
                    .prune_gs_unsafe(Key::message(hash_addr, topic_name_hash, index));
            }
            0
        } else {
            prev_topic_summary.message_count()
        };

        let block_message_index: u64 = match self
            .context
            .read_gs(&Key::BlockGlobal(BlockGlobalAddr::MessageCount))?
        {
            Some(stored_value) => {
                let (prev_block_time, prev_count): (BlockTime, u64) = CLValue::into_t(
                    CLValue::try_from(stored_value).map_err(ExecError::TypeMismatch)?,
                )
                .map_err(ExecError::CLValue)?;
                if prev_block_time == current_blocktime {
                    prev_count
                } else {
                    0
                }
            }
            None => 0,
        };

        let Some(topic_message_count) = topic_message_index.checked_add(1) else {
            return Ok(Err(ApiError::MessageTopicFull));
        };

        let Some(block_message_count) = block_message_index.checked_add(1) else {
            return Ok(Err(ApiError::MaxMessagesPerBlockExceeded));
        };

        self.context.metered_emit_message(
            topic_key,
            current_blocktime,
            block_message_count,
            topic_message_count,
            Message::new(
                hash_addr,
                message,
                topic_name.to_string(),
                topic_name_hash,
                topic_message_index,
                block_message_index,
            ),
        )?;
        Ok(Ok(()))
    }
}

#[cfg(feature = "test-support")]
fn dump_runtime_stack_info(instance: casper_wasmi::ModuleRef, max_stack_height: u32) {
    let globals = instance.globals();
    let Some(current_runtime_call_stack_height) = globals.last() else {
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
