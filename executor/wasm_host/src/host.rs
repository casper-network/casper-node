use std::{borrow::Cow, num::NonZeroU32, sync::Arc};

use bytes::Bytes;
use casper_executor_wasm_common::{
    chain_utils,
    entry_point::{
        ENTRY_POINT_PAYMENT_CALLER, ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY,
        ENTRY_POINT_PAYMENT_SELF_ONWARD,
    },
    env_info::EnvInfo,
    error::{
        CallError, CALLEE_NOT_CALLABLE, CALLEE_SUCCEEDED, CALLEE_TRAPPED, HOST_ERROR_INVALID_DATA,
        HOST_ERROR_INVALID_INPUT, HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED,
        HOST_ERROR_MESSAGE_TOPIC_FULL, HOST_ERROR_NOT_FOUND, HOST_ERROR_PAYLOAD_TOO_LONG,
        HOST_ERROR_SUCCESS, HOST_ERROR_TOO_MANY_TOPICS, HOST_ERROR_TOPIC_TOO_LONG,
    },
    flags::ReturnFlags,
    keyspace::{Keyspace, KeyspaceTag},
};
use casper_executor_wasm_interface::{
    executor::{ExecuteError, ExecuteRequestBuilder, ExecuteResult, ExecutionKind, Executor},
    u32_from_host_result, Caller, InternalHostError, VMError, VMResult,
};
use casper_storage::{
    global_state::GlobalStateReader,
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{
        ActionThresholds, AssociatedKeys, MessageTopicError, NamedKeyAddr, NamedKeyValue,
    },
    bytesrepr::{FromBytes, ToBytes},
    contract_messages::{Message, MessageAddr, MessagePayload, MessageTopicSummary},
    execution::RetValue,
    AddressableEntity, BlockGlobalAddr, BlockHash, BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash,
    ByteCodeKind, CLType, CLValue, ContractRuntimeTag, Digest, EntityAddr, EntityEntryPoint,
    EntityKind, EntryPointAccess, EntryPointAddr, EntryPointPayment, EntryPointType,
    EntryPointValue, HashAddr, HashAlgorithm, HostFunctionV2, Key, Package, PackageHash,
    ProtocolVersion, StoredValue, URef, U512,
};
use either::Either;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use tracing::{error, info, warn};

use blake2::{
    digest::{Update, VariableOutput},
    Blake2bVar,
};
use keccak_asm::Digest as KeccakDigest;
use sha2::Sha256;

use crate::{
    abi::{CreateResult, ReadInfo},
    context::Context,
    system::{self, DispatchError, MintTransferArgs},
};

#[derive(Debug, Copy, Clone, FromPrimitive, PartialEq)]
enum EntityKindTag {
    Account = 0,
    Contract = 1,
}

pub trait FallibleInto<T> {
    fn try_into_wrapped(self) -> VMResult<T>;
}

impl<From, To> FallibleInto<To> for From
where
    To: TryFrom<From>,
{
    fn try_into_wrapped(self) -> VMResult<To> {
        To::try_from(self).map_err(|_| VMError::Internal(InternalHostError::TypeConversion))
    }
}

/// Consumes a set amount of gas for the specified storage value.
fn charge_gas_storage<S: GlobalStateReader, E: Executor>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    size_bytes: usize,
) -> VMResult<()> {
    let storage_costs = &caller.context().storage_costs;
    let gas_cost = storage_costs.calculate_gas_cost(size_bytes);
    let value: u64 = gas_cost.value().try_into().map_err(|_| VMError::OutOfGas)?;
    caller.consume_gas(value)?;
    Ok(())
}

/// Consumes a set amount of gas for the specified host function and weights
fn charge_host_function_call<S, E, const N: usize>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    host_function: &HostFunctionV2<[u64; N]>,
    weights: [u64; N],
) -> VMResult<()>
where
    S: GlobalStateReader,
    E: Executor,
{
    let Some(cost) = host_function.calculate_gas_cost(weights) else {
        // Overflowing gas calculation means gas limit was exceeded
        return Err(VMError::OutOfGas);
    };

    caller.consume_gas(cost.value().as_u64())?;
    Ok(())
}

/// Writes a message to the global state and charges for storage used.
fn metered_write<S: GlobalStateReader, E: Executor>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    key: Key,
    value: StoredValue,
) -> VMResult<()> {
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    charge_gas_storage(caller, value.serialized_length())?;
    caller.context_mut().tracking_copy.write(key, value);
    Ok(())
}

/// Write value under a key.
pub fn casper_write<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
    value_ptr: u32,
    value_size: u32,
) -> VMResult<u32> {
    // In restricted mode, writing is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let write_cost = caller.context().config.host_function_costs().write;
    charge_host_function_call(
        &mut caller,
        &write_cost,
        [
            key_space,
            u64::from(key_ptr),
            u64::from(key_size),
            u64::from(value_ptr),
            u64::from(value_size),
        ],
    )?;

    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let key_payload_bytes = caller.memory_read(key_ptr, key_size.try_into_wrapped()?)?;

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    // TODO: Invalid key name encoding
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            Keyspace::NamedKey(key_name)
        }
        KeyspaceTag::PaymentInfo => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            if !caller.has_export(key_name)? {
                // Missing wasm export, unable to perform global state write
                return Ok(HOST_ERROR_NOT_FOUND);
            }

            Keyspace::PaymentInfo(key_name)
        }
    };

    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let value = caller.memory_read(value_ptr, value_size.try_into_wrapped()?)?;

    let stored_value = match keyspace {
        Keyspace::State | Keyspace::Context(_) => {
            let cl_value_any = CLValue::from_components(CLType::Any, value);
            StoredValue::CLValue(cl_value_any)
        }
        Keyspace::NamedKey(name) => {
            let key = match Key::from_bytes(&value) {
                Ok((key, remainder)) => {
                    if !remainder.is_empty() {
                        return Ok(HOST_ERROR_INVALID_INPUT);
                    }
                    key
                }
                Err(_) => return Ok(HOST_ERROR_INVALID_DATA),
            };

            let named_key_value = match NamedKeyValue::from_concrete_values(key, name.to_string()) {
                Ok(named_key_value) => named_key_value,
                Err(_) => return Ok(HOST_ERROR_INVALID_DATA),
            };
            StoredValue::NamedKey(named_key_value)
        }
        Keyspace::PaymentInfo(_) => {
            let entry_point_payment = match value.as_slice() {
                [ENTRY_POINT_PAYMENT_CALLER] => EntryPointPayment::Caller,
                [ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY] => {
                    EntryPointPayment::DirectInvocationOnly
                }
                [ENTRY_POINT_PAYMENT_SELF_ONWARD] => EntryPointPayment::SelfOnward,
                _ => {
                    // Invalid entry point payment variant
                    return Ok(HOST_ERROR_INVALID_INPUT);
                }
            };

            let entry_point = EntityEntryPoint::new(
                "_",
                Vec::new(),
                CLType::Unit,
                EntryPointAccess::Public,
                EntryPointType::Called,
                entry_point_payment,
            );
            let entry_point_value = EntryPointValue::V1CasperVm(entry_point);
            StoredValue::EntryPoint(entry_point_value)
        }
    };

    metered_write(&mut caller, global_state_key, stored_value)?;

    Ok(HOST_ERROR_SUCCESS)
}

/// Remove value under a key.
///
/// This produces a transformation of Prune to the global state. Keep in mind that technically the
/// data is not removed from the global state as it still there, it's just not reachable anymore
/// from the newly created tip.
///
/// The name for this host function is `remove` to keep it simple and consistent with read/write
/// verbs, and also consistent with the rust stdlib vocabulary i.e. `V`
pub fn casper_remove<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
) -> VMResult<u32> {
    // In restricted mode, removing is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let remove_cost = caller.context().config.host_function_costs().remove;
    charge_host_function_call(
        &mut caller,
        &remove_cost,
        [key_space, u64::from(key_ptr), u64::from(key_size)],
    )?;

    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let key_payload_bytes = caller.memory_read(key_ptr, key_size.try_into_wrapped()?)?;

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    // TODO: Invalid key name encoding
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            Keyspace::NamedKey(key_name)
        }
        KeyspaceTag::PaymentInfo => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            if !caller.has_export(key_name)? {
                // Missing wasm export, unable to perform global state write
                return Ok(HOST_ERROR_NOT_FOUND);
            }

            Keyspace::PaymentInfo(key_name)
        }
    };

    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let global_state_read_result = caller.context_mut().tracking_copy.read(&global_state_key);
    match global_state_read_result {
        Ok(Some(_stored_value)) => {
            // Produce a prune transform only if value under a given key exists in the global state
            caller.context_mut().tracking_copy.prune(global_state_key);
        }
        Ok(None) => {
            // Entry does not exist, and we can't proceed with the prune operation
            return Ok(HOST_ERROR_NOT_FOUND);
        }
        Err(error) => {
            // To protect the network against potential non-determinism (i.e. one validator runs out
            // of space or just faces I/O issues that other validators may not have) we're simply
            // aborting the process, hoping that once the node goes back online issues are resolved
            // on the validator side. TODO: We should signal this to the contract
            // runtime somehow, and let validator nodes skip execution.
            error!(
                ?error,
                ?global_state_key,
                "Error while attempting a read before removing value; aborting"
            );
            panic!("Error while attempting a read before removing value; aborting key={global_state_key:?} error={error:?}")
        }
    }

    Ok(HOST_ERROR_SUCCESS)
}

pub fn casper_print<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    message_ptr: u32,
    message_size: u32,
) -> VMResult<()> {
    let print_cost = caller.context().config.host_function_costs().print;
    charge_host_function_call(
        &mut caller,
        &print_cost,
        [u64::from(message_ptr), u64::from(message_size)],
    )?;

    let vec = caller.memory_read(message_ptr, message_size.try_into_wrapped()?)?;
    let msg = String::from_utf8_lossy(&vec);
    eprintln!("⛓️ {msg}");
    Ok(())
}

/// Write value under a key.
pub fn casper_read<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    key_tag: u64,
    key_ptr: u32,
    key_size: u32,
    info_ptr: u32,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    let read_cost = caller.context().config.host_function_costs().read;
    charge_host_function_call(
        &mut caller,
        &read_cost,
        [
            key_tag,
            u64::from(key_ptr),
            u64::from(key_size),
            u64::from(info_ptr),
            u64::from(cb_alloc),
            u64::from(alloc_ctx),
        ],
    )?;

    let keyspace_tag = match KeyspaceTag::from_u64(key_tag) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_INVALID_INPUT);
        }
    };

    // TODO: Opportunity for optimization: don't read data under key_ptr if given key space does not
    // require it.
    let key_payload_bytes = caller.memory_read(key_ptr, key_size.try_into_wrapped()?)?;

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };

            Keyspace::NamedKey(key_name)
        }
        KeyspaceTag::PaymentInfo => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok(HOST_ERROR_INVALID_DATA);
                }
            };
            if !caller.has_export(key_name)? {
                // Missing wasm export, unable to perform global state read
                return Ok(HOST_ERROR_NOT_FOUND);
            }
            Keyspace::PaymentInfo(key_name)
        }
    };

    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };
    let global_state_read_result = caller.context_mut().tracking_copy.read(&global_state_key);

    let global_state_raw_bytes: Cow<[u8]> = match global_state_read_result {
        Ok(Some(StoredValue::CLValue(cl_value))) => {
            let CLType::Any = cl_value.cl_type() else {
                return Err(InternalHostError::TypeConversion)?;
            };
            Cow::Owned(cl_value.inner_bytes().to_owned())
        }
        Ok(Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point)))) => {
            match entry_point.entry_point_payment() {
                EntryPointPayment::Caller => Cow::Borrowed(&[ENTRY_POINT_PAYMENT_CALLER]),
                EntryPointPayment::DirectInvocationOnly => {
                    Cow::Borrowed(&[ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY])
                }
                EntryPointPayment::SelfOnward => Cow::Borrowed(&[ENTRY_POINT_PAYMENT_SELF_ONWARD]),
            }
        }
        Ok(Some(stored_value)) => {
            // TODO: Backwards compatibility with old EE, although it's not clear if we should do it
            // at the storage level. Since new VM has storage isolated from the Wasm
            // (i.e. we have Keyspace on the wasm which gets converted to a global state `Key`).
            // I think if we were to pursue this we'd add a new `Keyspace` enum variant for each old
            // VM supported Key types (i.e. URef, Dictionary perhaps) for some period of time, then
            // deprecate this.
            todo!("Unsupported {stored_value:?}")
        }
        Ok(None) => return Ok(HOST_ERROR_NOT_FOUND), // Entry does not exist
        Err(error) => {
            // To protect the network against potential non-determinism (i.e. one validator runs out
            // of space or just faces I/O issues that other validators may not have) we're simply
            // aborting the process, hoping that once the node goes back online issues are resolved
            // on the validator side. TODO: We should signal this to the contract
            // runtime somehow, and let validator nodes skip execution.
            error!(?error, "Error while reading from storage; aborting");
            panic!("Error while reading from storage; aborting key={global_state_key:?} error={error:?}")
        }
    };

    let out_ptr: u32 = if cb_alloc != 0 {
        caller.alloc(cb_alloc, global_state_raw_bytes.len(), alloc_ctx)?
    } else {
        // treats alloc_ctx as data
        alloc_ctx
    };

    let read_info = ReadInfo {
        data: out_ptr,
        data_size: global_state_raw_bytes.len().try_into_wrapped()?,
    };

    let read_info_bytes = safe_transmute::transmute_one_to_bytes(&read_info);
    caller.memory_write(info_ptr, read_info_bytes)?;
    if out_ptr != 0 {
        caller.memory_write(out_ptr, &global_state_raw_bytes)?;
    }
    Ok(HOST_ERROR_SUCCESS)
}

fn keyspace_to_global_state_key<S: GlobalStateReader, E: Executor>(
    context: &Context<S, E>,
    keyspace: Keyspace<'_>,
) -> Option<Key> {
    let entity_addr = context_to_entity_addr(context);

    match keyspace {
        Keyspace::State => Some(Key::State(entity_addr)),
        Keyspace::Context(bytes) => {
            let digest = Digest::hash(bytes);
            Some(Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                entity_addr,
                digest.value(),
            )))
        }
        Keyspace::NamedKey(payload) => {
            let digest = Digest::hash(payload.as_bytes());
            Some(Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                entity_addr,
                digest.value(),
            )))
        }
        Keyspace::PaymentInfo(payload) => {
            let entry_point_addr =
                EntryPointAddr::new_v1_entry_point_addr(entity_addr, payload).ok()?;
            Some(Key::EntryPoint(entry_point_addr))
        }
    }
}

fn context_to_entity_addr<S: GlobalStateReader, E: Executor>(
    context: &Context<S, E>,
) -> EntityAddr {
    match context.callee {
        Key::Account(account_hash) => EntityAddr::new_account(account_hash.value()),
        Key::SmartContract(smart_contract_addr) => {
            EntityAddr::new_smart_contract(smart_contract_addr)
        }
        _ => {
            // This should never happen, as the caller is always an account or a smart contract.
            panic!("Unexpected callee variant: {:?}", context.callee)
        }
    }
}

pub fn casper_copy_input<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    let input = caller.context().input.clone();

    let out_ptr: u32 = if cb_alloc != 0 {
        caller.alloc(cb_alloc, input.len(), alloc_ctx)?
    } else {
        // treats alloc_ctx as data
        alloc_ctx
    };

    let copy_input_cost = caller.context().config.host_function_costs().copy_input;
    charge_host_function_call(
        &mut caller,
        &copy_input_cost,
        [
            u64::from(out_ptr),
            input.len().try_into().map_err(|err| {
                error!("Failed to convert u64 to usize. Details: {err}");
                ExecuteError::InternalHost(InternalHostError::TypeConversion)
            })?,
        ],
    )?;

    if out_ptr == 0 {
        Ok(out_ptr)
    } else {
        caller.memory_write(out_ptr, &input)?;
        Ok(out_ptr + (input.len() as u32))
    }
}

/// Returns from the execution of a smart contract with an optional flags.
pub fn casper_return<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    flags: u32,
    data_ptr: u32,
    data_len: u32,
) -> VMResult<()> {
    let ret_cost = caller.context().config.host_function_costs().ret;
    charge_host_function_call(
        &mut caller,
        &ret_cost,
        [u64::from(data_ptr), u64::from(data_len)],
    )?;

    let maybe_flags = ReturnFlags::from_bits(flags);
    let flags = match maybe_flags {
        Some(flags) => flags,
        None => {
            return VMResult::Err(VMError::Execute(ExecuteError::ReturnFlagsNotSupported(
                flags,
            )))
        }
    };
    let data = if data_ptr == 0 {
        None
    } else {
        let data = caller
            .memory_read(data_ptr, data_len.try_into_wrapped()?)
            .map(Bytes::from)?;

        let key = caller.context().callee;
        let bytes = casper_types::bytesrepr::Bytes::from(data.to_vec());
        caller
            .context_mut()
            .tracking_copy
            .ret(key, RetValue::Bytes(bytes));

        Some(data)
    };
    Err(VMError::Return { flags, data })
}

#[allow(clippy::too_many_arguments)]
pub fn casper_create<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    code_ptr: u32,
    code_len: u32,
    transferred_value: u64,
    entry_point_ptr: u32,
    entry_point_len: u32,
    input_ptr: u32,
    input_len: u32,
    seed_ptr: u32,
    seed_len: u32,
    result_ptr: u32,
) -> VMResult<u32> {
    // In restricted mode, contract creation is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let create_cost = caller.context().config.host_function_costs().create;
    charge_host_function_call(
        &mut caller,
        &create_cost,
        [
            u64::from(code_ptr),
            u64::from(code_len),
            transferred_value,
            u64::from(entry_point_ptr),
            u64::from(entry_point_len),
            u64::from(input_ptr),
            u64::from(input_len),
            u64::from(seed_ptr),
            u64::from(seed_len),
            u64::from(result_ptr),
        ],
    )?;

    let code = if code_ptr != 0 {
        caller
            .memory_read(code_ptr, code_len as usize)
            .map(Bytes::from)?
    } else {
        caller.bytecode()
    };

    let seed = if seed_ptr != 0 {
        if seed_len != 32 {
            return Ok(CALLEE_NOT_CALLABLE);
        }
        let seed_bytes = caller.memory_read(seed_ptr, seed_len as usize)?;
        let seed_bytes: [u8; 32] = seed_bytes.try_into().map_err(|_| {
            // SAFETY: We checked for length. This shouldn't happen
            error!("Error when converting seed_bytes from vec to static array");
            ExecuteError::InternalHost(InternalHostError::TypeConversion)
        })?;
        Some(seed_bytes)
    } else {
        None
    };

    // For calling a constructor
    let constructor_entry_point = {
        let entry_point_ptr = NonZeroU32::new(entry_point_ptr);
        match entry_point_ptr {
            Some(entry_point_ptr) => {
                let entry_point_bytes =
                    caller.memory_read(entry_point_ptr.get(), entry_point_len as _)?;
                match String::from_utf8(entry_point_bytes) {
                    Ok(entry_point) => Some(entry_point),
                    Err(utf8_error) => {
                        error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                        return Ok(CALLEE_NOT_CALLABLE);
                    }
                }
            }
            None => {
                // No constructor to be called
                None
            }
        }
    };

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> = if input_ptr == 0 {
        None
    } else {
        let input_data = caller.memory_read(input_ptr, input_len as _)?.into();
        Some(input_data)
    };

    let bytecode_hash = chain_utils::compute_wasm_bytecode_hash(&code);

    let bytecode = ByteCode::new(ByteCodeKind::V2CasperWasm, code.clone().into());
    let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash);

    // 1. Store package hash
    let mut smart_contract_package = Package::default();

    let protocol_version = ProtocolVersion::V2_0_0;
    let protocol_version_major = protocol_version.value().major;

    let callee_addr = context_to_entity_addr(caller.context()).value();

    let smart_contract_addr: HashAddr = chain_utils::compute_predictable_address(
        caller.context().chain_name.as_bytes(),
        callee_addr,
        bytecode_hash,
        seed,
    );

    smart_contract_package.insert_entity_version(
        protocol_version_major,
        EntityAddr::SmartContract(smart_contract_addr),
    );

    if caller
        .context_mut()
        .tracking_copy
        .read(&Key::SmartContract(smart_contract_addr))
        .map_err(|_| VMError::Internal(InternalHostError::TrackingCopy))?
        .is_some()
    {
        return Err(VMError::Internal(InternalHostError::ContractAlreadyExists));
    }

    metered_write(
        &mut caller,
        Key::SmartContract(smart_contract_addr),
        StoredValue::SmartContract(smart_contract_package),
    )?;

    // 2. Store wasm
    metered_write(
        &mut caller,
        Key::ByteCode(bytecode_addr),
        StoredValue::ByteCode(bytecode),
    )?;

    // 3. Store addressable entity

    let entity_addr = EntityAddr::SmartContract(smart_contract_addr);
    let addressable_entity_key = Key::AddressableEntity(entity_addr);

    // TODO: abort(str) as an alternative to trap
    let address_generator = Arc::clone(&caller.context().address_generator);
    let transaction_hash = caller.context().transaction_hash;
    let runtime_native_config = caller.context().runtime_native_config.clone();
    let main_purse: URef = match system::create_purse(
        &mut caller.context_mut().tracking_copy,
        runtime_native_config,
        transaction_hash,
        address_generator,
    ) {
        Ok(uref) => uref,
        Err(mint_error) => {
            error!(?mint_error, "Failed to create a purse");
            return Ok(CALLEE_TRAPPED);
        }
    };

    let addressable_entity = AddressableEntity::new(
        PackageHash::new(smart_contract_addr),
        ByteCodeHash::new(bytecode_hash),
        ProtocolVersion::V2_0_0,
        main_purse,
        AssociatedKeys::default(),
        ActionThresholds::default(),
        EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2),
    );

    metered_write(
        &mut caller,
        addressable_entity_key,
        StoredValue::AddressableEntity(addressable_entity),
    )?;

    let _initial_state = match constructor_entry_point {
        Some(entry_point_name) => {
            // Take the gas spent so far and use it as a limit for the new VM.
            let gas_limit = caller
                .gas_consumed()?
                .try_into_remaining()
                .map_err(|_| InternalHostError::TypeConversion)?;

            let execute_request = ExecuteRequestBuilder::default()
                .with_initiator(caller.context().initiator)
                .with_caller_key(caller.context().callee)
                .with_gas_limit(gas_limit)
                .with_target(ExecutionKind::Stored {
                    address: smart_contract_addr,
                    entry_point: entry_point_name.clone(),
                })
                .with_input(input_data.unwrap_or_default())
                .with_transferred_value(transferred_value)
                .with_transaction_hash(caller.context().transaction_hash)
                // We're using shared address generator there as we need to preserve and advance the
                // state of deterministic address generator across chain of calls.
                .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
                .with_chain_name(caller.context().chain_name.clone())
                .with_block_time(caller.context().block_time)
                .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
                .with_block_height(1) // TODO: Carry on block height
                .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
                .with_runtime_native_config(caller.context().runtime_native_config.clone())
                .build()
                .map_err(InternalHostError::ExecuteRequestBuildFailure)?;

            let tracking_copy_for_ctor = caller.context().tracking_copy.fork2();

            match caller
                .context()
                .executor
                .execute(tracking_copy_for_ctor, execute_request)
            {
                Ok(ExecuteResult {
                    host_error,
                    output,
                    gas_usage,
                    effects,
                    cache,
                    messages,
                }) => {
                    // output
                    caller.consume_gas(gas_usage.gas_spent())?;

                    if let Some(host_error) = host_error {
                        return Ok(host_error.into_u32());
                    }

                    caller
                        .context_mut()
                        .tracking_copy
                        .apply_changes(effects, cache, messages);

                    output
                }
                Err(execute_error) => {
                    // This is a bug in the EE, as it should have been caught during the preparation
                    // phase when the contract was stored in the global state.
                    error!(?execute_error, "Failed to execute constructor entry point");
                    return Err(VMError::Execute(execute_error));
                }
            }
        }
        None => None,
    };

    let create_result = CreateResult {
        package_address: smart_contract_addr,
    };

    let create_result_bytes = safe_transmute::transmute_one_to_bytes(&create_result);

    debug_assert_eq!(
        safe_transmute::transmute_one(create_result_bytes),
        Ok(create_result),
        "Sanity check", // NOTE: Remove these guards with sufficient test coverage
    );

    caller.memory_write(result_ptr, create_result_bytes)?;

    Ok(CALLEE_SUCCEEDED)
}

#[allow(clippy::too_many_arguments)]
pub fn casper_call<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    address_ptr: u32,
    address_len: u32,
    transferred_value: u64,
    entry_point_ptr: u32,
    entry_point_len: u32,
    input_ptr: u32,
    input_len: u32,
    cb_alloc: u32,
    cb_ctx: u32,
) -> VMResult<u32> {
    // In restricted mode, contract calls are not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let call_cost = caller.context().config.host_function_costs().call;
    charge_host_function_call(
        &mut caller,
        &call_cost,
        [
            u64::from(address_ptr),
            u64::from(address_len),
            transferred_value,
            u64::from(entry_point_ptr),
            u64::from(entry_point_len),
            u64::from(input_ptr),
            u64::from(input_len),
            u64::from(cb_alloc),
            u64::from(cb_ctx),
        ],
    )?;

    // 1. Look up address in the storage
    // 1a. if it's legacy contract, wire up old EE, pretend you're 1.x. Input data would be
    // "RuntimeArgs". Serialized output of the call has to be passed as output. Value is ignored as
    // you can't pass value (tokens) to called contracts. 1b. if it's new contract, wire up
    // another VM as according to the bytecode format. 2. Depends on the VM used (old or new) at
    // this point either entry point is validated (i.e. EE returned error) or will be validated as
    // for now. 3. If entry point is valid, call it, transfer the value, pass the input data. If
    // it's invalid, return error. 4. Output data is captured by calling `cb_alloc`.
    // let vm = VM::new();
    // vm.
    let address = caller.memory_read(address_ptr, address_len as _)?;
    let smart_contract_addr: HashAddr = address.try_into_wrapped()?;

    let input_data: Bytes = caller.memory_read(input_ptr, input_len as _)?.into();

    let entry_point = {
        let entry_point_bytes = caller.memory_read(entry_point_ptr, entry_point_len as _)?;
        match String::from_utf8(entry_point_bytes) {
            Ok(entry_point) => entry_point,
            Err(utf8_error) => {
                error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                return Ok(CALLEE_NOT_CALLABLE);
            }
        }
    };

    let tracking_copy = caller.context().tracking_copy.fork2();

    // Take the gas spent so far and use it as a limit for the new VM.
    let gas_limit = caller
        .gas_consumed()?
        .try_into_remaining()
        .map_err(|_| InternalHostError::TypeConversion)?;

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(caller.context().initiator)
        .with_caller_key(caller.context().callee)
        .with_gas_limit(gas_limit)
        .with_target(ExecutionKind::Stored {
            address: smart_contract_addr,
            entry_point: entry_point.clone(),
        })
        .with_transferred_value(transferred_value)
        .with_input(input_data)
        .with_transaction_hash(caller.context().transaction_hash)
        // We're using shared address generator there as we need to preserve and advance the state
        // of deterministic address generator across chain of calls.
        .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
        .with_chain_name(caller.context().chain_name.clone())
        .with_block_time(caller.context().block_time)
        .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
        .with_block_height(1) // TODO: Carry on block height
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
        .with_runtime_native_config(caller.context().runtime_native_config.clone())
        .build()
        .map_err(InternalHostError::ExecuteRequestBuildFailure)?;

    let (gas_usage, host_result) = match caller
        .context()
        .executor
        .execute(tracking_copy, execute_request)
    {
        Ok(ExecuteResult {
            host_error,
            output,
            gas_usage,
            effects,
            cache,
            messages,
        }) => {
            if let Some(output) = output {
                let out_ptr: u32 = if cb_alloc != 0 {
                    caller.alloc(cb_alloc, output.len(), cb_ctx)?
                } else {
                    // treats alloc_ctx as data
                    cb_ctx
                };

                if out_ptr != 0 {
                    caller.memory_write(out_ptr, &output)?;
                }
            }

            let host_result = match host_error {
                Some(host_error) => Err(host_error),
                None => {
                    caller
                        .context_mut()
                        .tracking_copy
                        .apply_changes(effects, cache, messages);
                    Ok(())
                }
            };

            (gas_usage, host_result)
        }
        Err(execute_error) => {
            error!(
                ?execute_error,
                ?smart_contract_addr,
                ?entry_point,
                "Failed to execute entry point"
            );
            return Err(VMError::Execute(execute_error));
        }
    };

    let gas_spent = gas_usage
        .gas_limit()
        .checked_sub(gas_usage.remaining_points())
        .ok_or(InternalHostError::RemainingGasExceedsGasLimit)?;

    caller.consume_gas(gas_spent)?;

    Ok(u32_from_host_result(host_result))
}

pub fn casper_env_balance<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    entity_kind: u32,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
    output_ptr: u32,
) -> VMResult<u32> {
    let balance_cost = caller.context().config.host_function_costs().env_balance;
    charge_host_function_call(
        &mut caller,
        &balance_cost,
        [
            u64::from(entity_kind),
            u64::from(entity_addr_ptr),
            u64::from(entity_addr_len),
            u64::from(output_ptr),
        ],
    )?;

    let entity_key = match EntityKindTag::from_u32(entity_kind) {
        Some(EntityKindTag::Account) => {
            if entity_addr_len != 32 {
                return Ok(HOST_ERROR_SUCCESS);
            }
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let account_hash: AccountHash = AccountHash::new(entity_addr.try_into_wrapped()?);

            let account_key = Key::Account(account_hash);
            match caller.context_mut().tracking_copy.read(&account_key) {
                Ok(Some(StoredValue::CLValue(clvalue))) => {
                    let addressable_entity_key = clvalue
                        .into_t::<Key>()
                        .map_err(|_| InternalHostError::TypeConversion)?;
                    Either::Right(addressable_entity_key)
                }
                Ok(Some(StoredValue::Account(account))) => Either::Left(account.main_purse()),
                Ok(Some(other_entity)) => {
                    error!("Unexpected entity type: {other_entity:?}");
                    return Err(InternalHostError::UnexpectedEntityKind.into());
                }
                Ok(None) => return Ok(HOST_ERROR_SUCCESS),
                Err(error) => {
                    error!("Error while reading from storage; aborting key={account_key:?} error={error:?}");
                    return Err(InternalHostError::TrackingCopy.into());
                }
            }
        }
        Some(EntityKindTag::Contract) => {
            if entity_addr_len != 32 {
                return Ok(HOST_ERROR_SUCCESS);
            }
            let hash_bytes = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let hash_bytes: [u8; 32] = hash_bytes.try_into().map_err(|_| {
                // SAFETY: We checked for length. This shouldn't happen
                error!("Error when converting hash_bytes from vec to static array");
                ExecuteError::InternalHost(InternalHostError::TypeConversion)
            })?;
            let smart_contract_key = Key::SmartContract(hash_bytes);
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressable_entity_hash) => {
                            let key = Key::AddressableEntity(EntityAddr::SmartContract(
                                addressable_entity_hash.value(),
                            ));
                            Either::Right(key)
                        }
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressable entity hash for contract"
                            );
                            return Ok(HOST_ERROR_SUCCESS);
                        }
                    }
                }
                Ok(Some(_)) => {
                    return Ok(HOST_ERROR_SUCCESS);
                }
                Ok(None) => {
                    // Not found, balance is 0
                    return Ok(HOST_ERROR_SUCCESS);
                }
                Err(error) => {
                    error!(
                        hash_bytes = base16::encode_lower(&hash_bytes),
                        ?error,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        None => return Ok(HOST_ERROR_SUCCESS),
    };

    let purse = match entity_key {
        Either::Left(main_purse) => main_purse,
        Either::Right(indirect_entity_key) => {
            match caller
                .context_mut()
                .tracking_copy
                .read(&indirect_entity_key)
            {
                Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
                    addressable_entity.main_purse()
                }
                Ok(Some(other_entity)) => {
                    panic!("Unexpected entity type: {other_entity:?}")
                }
                Ok(None) => panic!("Key not found while checking balance"), //return Ok(0),
                Err(error) => {
                    panic!("Error while reading from storage; aborting key={entity_key:?} error={error:?}")
                }
            }
        }
    };

    let total_balance = caller
        .context_mut()
        .tracking_copy
        .get_total_balance(Key::URef(purse))
        .map_err(|_| InternalHostError::TotalBalanceReadFailure)?;

    let total_balance: u64 = total_balance
        .value()
        .try_into()
        .map_err(|_| InternalHostError::TotalBalanceOverflow)?;

    caller.memory_write(output_ptr, &total_balance.to_le_bytes())?;
    Ok(HOST_ERROR_NOT_FOUND)
}

pub fn casper_transfer<S: GlobalStateReader + 'static, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
    amount_ptr: u32,
) -> VMResult<u32> {
    // In restricted mode, transfers are not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let transfer_cost = caller.context().config.host_function_costs().transfer;
    charge_host_function_call(
        &mut caller,
        &transfer_cost,
        [
            u64::from(entity_addr_ptr),
            u64::from(entity_addr_len),
            u64::from(amount_ptr),
        ],
    )?;

    if entity_addr_len != 32 {
        // Invalid entity address; failing to proceed with the transfer
        return Ok(u32_from_host_result(Err(CallError::NotCallable)));
    }

    let amount = {
        let mut amount_bytes = [0u8; 8];
        caller.memory_read_into(amount_ptr, &mut amount_bytes)?;
        u64::from_le_bytes(amount_bytes)
    };

    let (target_entity_addr, _runtime_footprint) = {
        let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
        debug_assert_eq!(entity_addr.len(), 32);
        let account_hash: AccountHash = AccountHash::new(entity_addr.try_into().map_err(|_| {
            // SAFETY: We checked for length (32 bytes). This shouldn't happen
            error!("Error when converting entity_addr from vec to account_hash");
            ExecuteError::InternalHost(InternalHostError::TypeConversion)
        })?);

        let protocol_version = ProtocolVersion::V2_0_0;
        let (entity_addr, runtime_footprint) = match caller
            .context_mut()
            .tracking_copy
            .runtime_footprint_by_account_hash(protocol_version, account_hash)
        {
            Ok((entity_addr, runtime_footprint)) => (entity_addr, runtime_footprint),
            Err(TrackingCopyError::KeyNotFound(key)) => {
                warn!(?key, "Account not found");
                return Ok(u32_from_host_result(Err(CallError::NotCallable)));
            }
            Err(error) => {
                error!(?error, "Error while reading from storage; aborting");
                panic!("Error while reading from storage")
            }
        };
        (entity_addr, runtime_footprint)
    };

    let callee_addressable_entity_key = match caller.context().callee {
        callee_account_key @ Key::Account(_account_hash) => {
            match caller.context_mut().tracking_copy.read(&callee_account_key) {
                Ok(Some(StoredValue::CLValue(indirect))) => {
                    // is it an account?
                    indirect
                        .into_t::<Key>()
                        .map_err(|_| InternalHostError::TypeConversion)?
                }
                Ok(Some(other)) => panic!("should be cl value but got {other:?}"),
                Ok(None) => return Ok(u32_from_host_result(Err(CallError::NotCallable))),
                Err(error) => {
                    error!(
                        ?error,
                        ?callee_account_key,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        smart_contract_key @ Key::SmartContract(_) => {
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressable_entity_hash) => Key::AddressableEntity(
                            EntityAddr::SmartContract(addressable_entity_hash.value()),
                        ),
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressable entity hash for contract"
                            );
                            return Ok(u32_from_host_result(Err(CallError::NotCallable)));
                        }
                    }
                }
                Ok(Some(other)) => panic!("should be smart contract but got {other:?}"),
                Ok(None) => return Ok(u32_from_host_result(Err(CallError::NotCallable))),
                Err(error) => {
                    error!(
                        ?error,
                        ?smart_contract_key,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        other => panic!("should be account or smart contract but got {other:?}"),
    };

    let callee_stored_value = caller
        .context_mut()
        .tracking_copy
        .read(&callee_addressable_entity_key)
        .map_err(|_| InternalHostError::TrackingCopy)?;

    let callee_stored_value = match callee_stored_value {
        Some(callee_stored_value) => callee_stored_value,
        None => {
            warn!(
                ?callee_addressable_entity_key,
                "Callee not found while transferring tokens"
            );
            return Ok(u32_from_host_result(Err(CallError::NotCallable)));
        }
    };

    let callee_addressable_entity = callee_stored_value
        .into_addressable_entity()
        .ok_or(InternalHostError::TypeConversion)?;
    let callee_purse = callee_addressable_entity.main_purse();

    let target_purse = match caller
        .context_mut()
        .tracking_copy
        .runtime_footprint_by_entity_addr(target_entity_addr)
    {
        Ok(runtime_footprint) => match runtime_footprint.main_purse() {
            Some(target_purse) => target_purse,
            None => todo!("create a main purse for a contract"),
        },
        Err(TrackingCopyError::KeyNotFound(key)) => {
            warn!(?key, "Transfer recipient not found");
            return Ok(u32_from_host_result(Err(CallError::NotCallable)));
        }
        Err(error) => {
            error!(?error, "Error while reading from storage; aborting");
            return Err(InternalHostError::TrackingCopy)?;
        }
    };
    // We don't execute anything as it does not make sense to execute an account as there
    // are no entry points.
    let transaction_hash = caller.context().transaction_hash;
    let address_generator = Arc::clone(&caller.context().address_generator);
    let runtime_native_config = caller.context().runtime_native_config.clone();

    let args = MintTransferArgs::new_simple(callee_purse, target_purse, U512::from(amount));

    match system::transfer(
        &mut caller.context_mut().tracking_copy,
        runtime_native_config,
        transaction_hash,
        address_generator,
        args,
    ) {
        Ok(()) => Ok(HOST_ERROR_SUCCESS),
        Err(DispatchError::Internal(internal_error)) => Err(VMError::Internal(internal_error)),
        Err(DispatchError::Call(call_error)) => {
            // This is a bug in the EE, as it should have been caught during the preparation phase
            // when the contract was stored in the global state.
            error!(?call_error, "Failed to transfer");
            Ok(call_error.into_u32())
        }
        Err(dispatch_error) => {
            error!(
                ?dispatch_error,
                "Failed to dispatch system contract while transferring tokens"
            );
            Err(VMError::Internal(InternalHostError::DispatchSystemContract))
        }
    }
}

pub fn casper_upgrade<S: GlobalStateReader + 'static, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    code_ptr: u32,
    code_size: u32,
    entry_point_ptr: u32,
    entry_point_size: u32,
    input_ptr: u32,
    input_size: u32,
) -> VMResult<u32> {
    // In restricted mode, contract upgrades are not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    let upgrade_cost = caller.context().config.host_function_costs().upgrade;
    charge_host_function_call(
        &mut caller,
        &upgrade_cost,
        [
            u64::from(code_ptr),
            u64::from(code_size),
            u64::from(entry_point_ptr),
            u64::from(entry_point_size),
            u64::from(input_ptr),
            u64::from(input_size),
        ],
    )?;

    let code = caller
        .memory_read(code_ptr, code_size as usize)
        .map(Bytes::from)?;

    let entry_point = match NonZeroU32::new(entry_point_ptr) {
        Some(entry_point_ptr) => {
            // There's upgrade entry point to be called
            let entry_point_bytes =
                caller.memory_read(entry_point_ptr.get(), entry_point_size as usize)?;
            match String::from_utf8(entry_point_bytes) {
                Ok(entry_point) => Some(entry_point),
                Err(utf8_error) => {
                    error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                    return Ok(CALLEE_NOT_CALLABLE);
                }
            }
        }
        None => {
            // No constructor to be called
            None
        }
    };

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> = if input_ptr == 0 {
        None
    } else {
        let input_data = caller.memory_read(input_ptr, input_size as _)?.into();
        Some(input_data)
    };

    let (smart_contract_addr, callee_addressable_entity_key) = match caller.context().callee {
        Key::Account(_account_hash) => {
            error!("Account upgrade is not possible");
            return Ok(CALLEE_NOT_CALLABLE);
        }
        addressable_entity_key @ Key::SmartContract(smart_contract_addr) => {
            let smart_contract_key = addressable_entity_key;
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressable_entity_hash) => {
                            let key = Key::AddressableEntity(EntityAddr::SmartContract(
                                addressable_entity_hash.value(),
                            ));
                            (smart_contract_addr, key)
                        }
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressable entity hash for contract"
                            );
                            return Ok(CALLEE_NOT_CALLABLE);
                        }
                    }
                }
                Ok(Some(other)) => panic!("should be smart contract but got {other:?}"),
                Ok(None) => return Ok(CALLEE_NOT_CALLABLE),
                Err(error) => {
                    error!(
                        ?error,
                        ?smart_contract_key,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        other => panic!("should be account or addressable entity but got {other:?}"),
    };

    let callee_addressable_entity = match caller
        .context_mut()
        .tracking_copy
        .read(&callee_addressable_entity_key)
    {
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => addressable_entity,
        Ok(Some(other_entity)) => {
            panic!("Unexpected entity type: {other_entity:?}")
        }
        Ok(None) => return Ok(CALLEE_NOT_CALLABLE),
        Err(error) => {
            panic!("Error while reading from storage; aborting key={callee_addressable_entity_key:?} error={error:?}")
        }
    };

    // 1. Ensure that the new code is valid (maybe?)
    // TODO: Is validating new code worth it if the user pays for the storage anyway? Should we
    // protect users against invalid code?

    // 2. Update the code therefore making hash(new_code) != addressable_entity.bytecode_addr (aka
    //    hash(old_code))
    let bytecode_key = Key::ByteCode(ByteCodeAddr::V2CasperWasm(
        callee_addressable_entity.byte_code_addr(),
    ));
    metered_write(
        &mut caller,
        bytecode_key,
        StoredValue::ByteCode(ByteCode::new(
            ByteCodeKind::V2CasperWasm,
            code.clone().into(),
        )),
    )?;

    // 3. Execute upgrade routine (if specified)
    // this code should handle reading old state, and saving new state

    if let Some(entry_point_name) = entry_point {
        // Take the gas spent so far and use it as a limit for the new VM.
        let gas_limit = caller
            .gas_consumed()?
            .try_into_remaining()
            .map_err(|_| InternalHostError::TypeConversion)?;

        let execute_request = ExecuteRequestBuilder::default()
            .with_initiator(caller.context().initiator)
            .with_caller_key(caller.context().callee)
            .with_gas_limit(gas_limit)
            .with_target(ExecutionKind::Stored {
                address: smart_contract_addr,
                entry_point: entry_point_name.clone(),
            })
            .with_input(input_data.unwrap_or_default())
            // Upgrade entry point is executed with zero value as it does not seem to make sense to
            // be able to transfer anything.
            .with_transferred_value(0)
            .with_transaction_hash(caller.context().transaction_hash)
            // We're using shared address generator there as we need to preserve and advance the
            // state of deterministic address generator across chain of calls.
            .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
            .with_chain_name(caller.context().chain_name.clone())
            .with_block_time(caller.context().block_time)
            .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
            .with_block_height(1) // TODO: Carry on block height
            .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
            .with_runtime_native_config(caller.context().runtime_native_config.clone())
            .build()
            .map_err(InternalHostError::ExecuteRequestBuildFailure)?;

        let tracking_copy_for_ctor = caller.context().tracking_copy.fork2();

        match caller
            .context()
            .executor
            .execute(tracking_copy_for_ctor, execute_request)
        {
            Ok(ExecuteResult {
                host_error,
                output,
                gas_usage,
                effects,
                cache,
                messages,
            }) => {
                // output
                caller.consume_gas(gas_usage.gas_spent())?;

                if let Some(host_error) = host_error {
                    return Ok(host_error.into_u32());
                }

                caller
                    .context_mut()
                    .tracking_copy
                    .apply_changes(effects, cache, messages);

                if let Some(output) = output {
                    info!(
                        ?entry_point_name,
                        ?output,
                        "unexpected output from migration entry point"
                    );
                }
            }
            Err(execute_error) => {
                // Unable to call contract because of execution error or internal host error.
                // This usually means an internal error that should not happen and has to be handled
                // by the contract runtime.
                error!(
                    ?execute_error,
                    ?entry_point_name,
                    smart_contract_addr = base16::encode_lower(&smart_contract_addr),
                    "Failed to execute upgrade entry point"
                );
                return Err(VMError::Execute(execute_error));
            }
        }
    }

    Ok(CALLEE_SUCCEEDED)
}

pub fn casper_env_info<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    info_ptr: u32,
    info_size: u32,
) -> VMResult<u32> {
    let block_time_cost = caller.context().config.host_function_costs().env_info;
    charge_host_function_call(
        &mut caller,
        &block_time_cost,
        [u64::from(info_ptr), u64::from(info_size)],
    )?;

    let (caller_kind, caller_addr) = match &caller.context().caller {
        Key::Account(account_hash) => (EntityKindTag::Account as u32, account_hash.value()),
        Key::SmartContract(smart_contract_addr) => {
            (EntityKindTag::Contract as u32, *smart_contract_addr)
        }
        other => panic!("Unexpected caller: {other:?}"),
    };

    let (callee_kind, callee_addr) = match &caller.context().callee {
        Key::Account(initiator_addr) => (EntityKindTag::Account as u32, initiator_addr.value()),
        Key::SmartContract(smart_contract_addr) => {
            (EntityKindTag::Contract as u32, *smart_contract_addr)
        }
        other => panic!("Unexpected callee: {other:?}"),
    };

    let transferred_value = caller.context().transferred_value;

    let block_time = caller.context().block_time.value();

    // `EnvInfo` in little-endian representation.
    let env_info_le = EnvInfo {
        caller_addr,
        caller_kind: caller_kind.to_le(),
        callee_addr,
        callee_kind: callee_kind.to_le(),
        transferred_value: transferred_value.to_le(),
        block_time: block_time.to_le(),
    };

    let env_info_bytes = safe_transmute::transmute_one_to_bytes(&env_info_le);
    let write_len = env_info_bytes.len().min(info_size as usize);
    caller.memory_write(info_ptr, &env_info_bytes[..write_len])?;

    Ok(HOST_ERROR_SUCCESS)
}

pub fn casper_emit<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    topic_name_ptr: u32,
    topic_name_size: u32,
    payload_ptr: u32,
    payload_size: u32,
) -> VMResult<u32> {
    // In restricted mode, emitting messages is not allowed
    if caller.context().sandboxed {
        return Err(InternalHostError::AttemptWriteInRestricted.into());
    }

    // Charge for parameter weights.
    let emit_host_function = caller.context().config.host_function_costs().emit;

    charge_host_function_call(
        &mut caller,
        &emit_host_function,
        [
            u64::from(topic_name_ptr),
            u64::from(topic_name_size),
            u64::from(payload_ptr),
            u64::from(payload_size),
        ],
    )?;

    if topic_name_size > caller.context().message_limits.max_topic_name_size {
        return Ok(HOST_ERROR_TOPIC_TOO_LONG);
    }

    if payload_size > caller.context().message_limits.max_message_size {
        return Ok(HOST_ERROR_PAYLOAD_TOO_LONG);
    }

    let topic_name = {
        let topic: Vec<u8> = caller.memory_read(topic_name_ptr, topic_name_size as usize)?;
        let Ok(topic) = String::from_utf8(topic) else {
            // Not a valid UTF-8 string
            return Ok(HOST_ERROR_INVALID_DATA);
        };
        topic
    };

    let payload = caller.memory_read(payload_ptr, payload_size as usize)?;

    let entity_addr = context_to_entity_addr(caller.context());

    let mut message_topics = caller
        .context_mut()
        .tracking_copy
        .get_message_topics(entity_addr)
        .unwrap_or_else(|error| {
            panic!("Error while reading from storage; aborting error={error:?}")
        });

    if message_topics.len() >= caller.context().message_limits.max_topics_per_contract as usize {
        return Ok(HOST_ERROR_TOO_MANY_TOPICS);
    }

    let topic_name_hash = Digest::hash(&topic_name).value().into();

    match message_topics.add_topic(&topic_name, topic_name_hash) {
        Ok(()) => {
            // New topic is created
        }
        Err(MessageTopicError::DuplicateTopic) => {
            // We're lazily creating message topics and this operation is idempotent.
            // Therefore, already existing topic is not an issue.
        }
        Err(MessageTopicError::MaxTopicsExceeded) => {
            // We're validating the size of topics before adding them
            return Ok(HOST_ERROR_TOO_MANY_TOPICS);
        }
        Err(MessageTopicError::TopicNameSizeExceeded) => {
            // We're validating the length of topic before adding it
            return Ok(HOST_ERROR_TOPIC_TOO_LONG);
        }
        Err(error) => {
            // These error variants are non_exhaustive, and we should handle them explicitly.
            unreachable!("Unexpected error while adding a topic: {:?}", error);
        }
    };

    let current_block_time = caller.context().block_time;
    eprintln!("📩 {topic_name}: {payload:?} (at {current_block_time:?})");

    let topic_key = Key::Message(MessageAddr::new_topic_addr(entity_addr, topic_name_hash));
    let prev_topic_summary = match caller.context_mut().tracking_copy.read(&topic_key) {
        Ok(Some(StoredValue::MessageTopic(message_topic_summary))) => message_topic_summary,
        Ok(Some(stored_value)) => {
            panic!("Unexpected stored value: {stored_value:?}");
        }
        Ok(None) => {
            let message_topic_summary =
                MessageTopicSummary::new(0, current_block_time, topic_name.clone());
            let summary = StoredValue::MessageTopic(message_topic_summary.clone());
            caller.context_mut().tracking_copy.write(topic_key, summary);
            message_topic_summary
        }
        Err(error) => panic!("Error while reading from storage; aborting error={error:?}"),
    };

    let topic_message_index = if prev_topic_summary.blocktime() != current_block_time {
        for index in 1..prev_topic_summary.message_count() {
            let message_key = Key::message(entity_addr, topic_name_hash, index);
            debug_assert!(
                {
                    // NOTE: This assertion is to ensure that the message index is continuous, and
                    // the previous messages are pruned properly.
                    caller
                        .context_mut()
                        .tracking_copy
                        .read(&message_key)
                        .map_err(|_| VMError::Internal(InternalHostError::TrackingCopy))?
                        .is_some()
                },
                "Message index is not continuous"
            );

            // Prune the previous messages
            caller.context_mut().tracking_copy.prune(message_key);
        }
        0
    } else {
        prev_topic_summary.message_count()
    };

    // Data stored in the global state associated with the message block.
    type MessageCountPair = (BlockTime, u64);

    let block_message_index: u64 = match caller
        .context_mut()
        .tracking_copy
        .read(&Key::BlockGlobal(BlockGlobalAddr::MessageCount))
    {
        Ok(Some(StoredValue::CLValue(value_pair))) => {
            let (prev_block_time, prev_count): MessageCountPair =
                CLValue::into_t(value_pair).map_err(|_| InternalHostError::TypeConversion)?;
            if prev_block_time == current_block_time {
                prev_count
            } else {
                0
            }
        }
        Ok(Some(other)) => panic!("Unexpected stored value: {other:?}"),
        Ok(None) => {
            // No messages in current block yet
            0
        }
        Err(error) => {
            panic!("Error while reading from storage; aborting error={error:?}")
        }
    };

    let Some(topic_message_count) = topic_message_index.checked_add(1) else {
        return Ok(HOST_ERROR_MESSAGE_TOPIC_FULL);
    };

    let Some(block_message_count) = block_message_index.checked_add(1) else {
        return Ok(HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED);
    };

    // Under v2 runtime messages are only limited to bytes.
    let message_payload = MessagePayload::Bytes(payload.into());

    let message = Message::new(
        entity_addr,
        message_payload,
        topic_name,
        topic_name_hash,
        topic_message_index,
        block_message_index,
    );
    let topic_value = StoredValue::MessageTopic(MessageTopicSummary::new(
        topic_message_count,
        current_block_time,
        message.topic_name().to_owned(),
    ));

    let message_key = message.message_key();
    let message_value = StoredValue::Message(
        message
            .checksum()
            .map_err(|_| InternalHostError::MessageChecksumMissing)?,
    );
    let message_count_pair: MessageCountPair = (current_block_time, block_message_count);
    let block_message_count_value = StoredValue::CLValue(
        CLValue::from_t(message_count_pair).map_err(|_| InternalHostError::TypeConversion)?,
    );

    // Charge for amount as measured by serialized length
    let bytes_count = topic_value.serialized_length()
        + message_value.serialized_length()
        + block_message_count_value.serialized_length();
    charge_gas_storage(&mut caller, bytes_count)?;

    caller.context_mut().tracking_copy.emit_message(
        topic_key,
        topic_value,
        message_key,
        message_value,
        block_message_count_value,
        message,
    );

    Ok(HOST_ERROR_SUCCESS)
}

/// Computes digest hash, using provided algorithm type.
///
/// # Arguments
///
/// * `in_ptr` - pointer to the location where argument bytes will be copied from the host side
/// * `in_size` - size of output pointer
/// * `out_ptr` - pointer to the location where argument bytes will be copied to the host side
/// * `hash_algo_type` - integer representation of HashAlgorithm enum variant
pub fn casper_generic_hash<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    in_ptr: u32,
    in_size: u32,
    hash_algorithm: u32,
    out_ptr: u32,
) -> VMResult<u32> {
    const DIGEST_LENGTH: usize = 32;

    let in_bytes: Vec<u8> = caller.memory_read(in_ptr, in_size as usize)?;

    // Charge for parameter weights.
    let generic_hash_host_function = caller.context().config.host_function_costs().generic_hash;

    charge_host_function_call(
        &mut caller,
        &generic_hash_host_function,
        [
            u64::from(in_ptr),
            u64::from(in_size),
            u64::from(out_ptr),
            u64::from(hash_algorithm),
        ],
    )?;

    let hash_algorithm =
        HashAlgorithm::from_u32(hash_algorithm).ok_or(InternalHostError::TypeConversion)?;

    let hashed_bytes = match hash_algorithm {
        HashAlgorithm::Blake2b => {
            let mut result = [0; DIGEST_LENGTH];
            let mut hasher = Blake2bVar::new(DIGEST_LENGTH).map_err(|_| {
                ExecuteError::InternalHost(InternalHostError::CorruptExecutionState(
                    "Error when creating instance of Blake2bVar hashing".to_owned(),
                ))
            })?;
            hasher.update(in_bytes.as_ref());
            hasher.finalize_variable(&mut result).ok();
            result
        }
        HashAlgorithm::Blake3 => {
            let mut result = [0; DIGEST_LENGTH];
            let mut hasher = blake3::Hasher::new();
            hasher.update(in_bytes.as_ref());
            let hash = hasher.finalize();
            let hash_bytes: &[u8; DIGEST_LENGTH] = hash.as_bytes();
            result.copy_from_slice(hash_bytes);
            result
        }
        HashAlgorithm::Sha256 => Sha256::digest(in_bytes).into(),
        HashAlgorithm::Keccak256 => {
            use keccak_asm::Keccak256;
            let mut result = [0u8; DIGEST_LENGTH];
            let mut hasher = Keccak256::new();
            KeccakDigest::update(&mut hasher, &in_bytes);
            let hash = KeccakDigest::finalize(hasher);
            result.copy_from_slice(&hash);
            result
        }
    };

    caller.memory_write(out_ptr, &hashed_bytes)?;

    Ok(HOST_ERROR_SUCCESS)
}
