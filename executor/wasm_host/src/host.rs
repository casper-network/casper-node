use std::{borrow::Cow, cmp, num::NonZeroU32, sync::Arc};

use bytes::Bytes;
use casper_executor_wasm_common::{
    chain_utils,
    entry_point::{
        ENTRY_POINT_PAYMENT_CALLER, ENTRY_POINT_PAYMENT_DIRECT_INVOCATION_ONLY,
        ENTRY_POINT_PAYMENT_SELF_ONWARD,
    },
    error::{
        CallError, TrapCode, HOST_ERROR_INVALID_DATA, HOST_ERROR_INVALID_INPUT,
        HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED, HOST_ERROR_MESSAGE_TOPIC_FULL,
        HOST_ERROR_NOT_FOUND, HOST_ERROR_PAYLOAD_TOO_LONG, HOST_ERROR_SUCCESS,
        HOST_ERROR_TOO_MANY_TOPICS, HOST_ERROR_TOPIC_TOO_LONG,
    },
    flags::ReturnFlags,
    keyspace::{Keyspace, KeyspaceTag},
};
use casper_executor_wasm_interface::{
    executor::{ExecuteError, ExecuteRequestBuilder, ExecuteResult, ExecutionKind, Executor},
    u32_from_host_result, Caller, HostResult, VMError, VMResult,
};
use casper_storage::{
    global_state::GlobalStateReader,
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, MessageTopicError, NamedKeyAddr},
    bytesrepr::ToBytes,
    contract_messages::{Message, MessageAddr, MessagePayload, MessageTopicSummary},
    AddressableEntity, BlockGlobalAddr, BlockHash, BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash,
    ByteCodeKind, CLType, CLValue, ContractRuntimeTag, Digest, EntityAddr, EntityEntryPoint,
    EntityKind, EntryPointAccess, EntryPointAddr, EntryPointPayment, EntryPointType,
    EntryPointValue, HashAddr, HostFunction, HostFunctionCost, Key, Package, PackageHash,
    ProtocolVersion, StoredValue, URef, U512,
};
use either::Either;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use tracing::{error, info, warn};

use crate::{
    abi::{CreateResult, ReadInfo},
    context::Context,
    system::{self, MintArgs, MintTransferArgs},
};

#[derive(Debug, Copy, Clone, FromPrimitive, PartialEq)]
enum EntityKindTag {
    Account = 0,
    Contract = 1,
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
fn charge_host_function_call<S, E, T>(
    caller: &mut impl Caller<Context = Context<S, E>>,
    host_function: &HostFunction<T>,
    weights: T,
) -> VMResult<()>
where
    S: GlobalStateReader,
    E: Executor,
    T: AsRef<[HostFunctionCost]> + Copy,
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
    let write_cost = caller.context().config.host_function_costs().write;
    charge_host_function_call(
        &mut caller,
        &write_cost,
        [key_space as u32, key_ptr, key_size, value_ptr, value_size],
    )?;

    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(HOST_ERROR_NOT_FOUND);
        }
    };

    let key_payload_bytes = caller.memory_read(key_ptr, key_size.try_into().unwrap())?;

    let keyspace = match keyspace_tag {
        KeyspaceTag::State => Keyspace::State,
        KeyspaceTag::Context => Keyspace::Context(&key_payload_bytes),
        KeyspaceTag::NamedKey => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    // TODO: Invalid key name encoding
                    return Ok(HOST_ERROR_NOT_FOUND);
                }
            };

            Keyspace::NamedKey(key_name)
        }
        KeyspaceTag::PaymentInfo => {
            let key_name = match std::str::from_utf8(&key_payload_bytes) {
                Ok(key_name) => key_name,
                Err(_) => {
                    return Ok(1);
                }
            };

            if !caller.has_export(key_name) {
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

    let value = caller.memory_read(value_ptr, value_size.try_into().unwrap())?;

    let stored_value = match keyspace {
        Keyspace::State | Keyspace::Context(_) | Keyspace::NamedKey(_) => {
            StoredValue::RawBytes(value)
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

    Ok(0)
}

pub fn casper_print<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    message_ptr: u32,
    message_size: u32,
) -> VMResult<()> {
    let print_cost = caller.context().config.host_function_costs().print;
    charge_host_function_call(&mut caller, &print_cost, [message_ptr, message_size])?;

    let vec = caller.memory_read(message_ptr, message_size.try_into().unwrap())?;
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
) -> Result<u32, VMError> {
    let read_cost = caller.context().config.host_function_costs().read;
    charge_host_function_call(
        &mut caller,
        &read_cost,
        [
            key_tag as u32,
            key_ptr,
            key_size,
            info_ptr,
            cb_alloc,
            alloc_ctx,
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
    let key_payload_bytes = caller.memory_read(key_ptr, key_size.try_into().unwrap())?;

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
            if !caller.has_export(key_name) {
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
        Ok(Some(StoredValue::RawBytes(raw_bytes))) => Cow::Owned(raw_bytes),
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
        Ok(None) => return Ok(HOST_ERROR_NOT_FOUND), // Entry does not exists
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
        data_size: global_state_raw_bytes.len().try_into().unwrap(),
    };

    let read_info_bytes = safe_transmute::transmute_one_to_bytes(&read_info);
    caller.memory_write(info_ptr, read_info_bytes)?;
    if out_ptr != 0 {
        caller.memory_write(out_ptr, &global_state_raw_bytes)?;
    }
    Ok(0)
}

fn keyspace_to_global_state_key<S: GlobalStateReader, E: Executor>(
    context: &Context<S, E>,
    keyspace: Keyspace<'_>,
) -> Option<Key> {
    let entity_addr = context_to_entity_addr(context);

    match keyspace {
        Keyspace::State => Some(Key::State(entity_addr)),
        Keyspace::Context(payload) => {
            let digest = Digest::hash(payload);
            Some(casper_types::Key::NamedKey(
                NamedKeyAddr::new_named_key_entry(entity_addr, digest.value()),
            ))
        }
        Keyspace::NamedKey(payload) => {
            let digest = Digest::hash(payload.as_bytes());
            Some(casper_types::Key::NamedKey(
                NamedKeyAddr::new_named_key_entry(entity_addr, digest.value()),
            ))
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
    charge_host_function_call(&mut caller, &copy_input_cost, [out_ptr, input.len() as u32])?;

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
    charge_host_function_call(&mut caller, &ret_cost, [data_ptr, data_len])?;

    let flags = ReturnFlags::from_bits_retain(flags);
    let data = if data_ptr == 0 {
        None
    } else {
        let data = caller
            .memory_read(data_ptr, data_len.try_into().unwrap())
            .map(Bytes::from)?;
        Some(data)
    };
    Err(VMError::Return { flags, data })
}

#[allow(clippy::too_many_arguments)]
pub fn casper_create<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    code_ptr: u32,
    code_len: u32,
    value_ptr: u32,
    entry_point_ptr: u32,
    entry_point_len: u32,
    input_ptr: u32,
    input_len: u32,
    seed_ptr: u32,
    seed_len: u32,
    result_ptr: u32,
) -> VMResult<HostResult> {
    let create_cost = caller.context().config.host_function_costs().create;
    charge_host_function_call(
        &mut caller,
        &create_cost,
        [
            code_ptr,
            code_len,
            value_ptr,
            entry_point_ptr,
            entry_point_len,
            input_ptr,
            input_len,
            seed_ptr,
            seed_len,
            result_ptr,
        ],
    )?;

    let code = if code_ptr != 0 {
        caller
            .memory_read(code_ptr, code_len as usize)
            .map(Bytes::from)?
    } else {
        caller.bytecode()
    };

    let value: u128 = {
        let mut value_bytes = [0u8; 16];
        caller.memory_read_into(value_ptr, &mut value_bytes)?;
        u128::from_le_bytes(value_bytes)
    };

    let seed = if seed_ptr != 0 {
        if seed_len != 32 {
            return Ok(Err(CallError::NotCallable));
        }
        let seed_bytes = caller.memory_read(seed_ptr, seed_len as usize)?;
        let seed_bytes: [u8; 32] = seed_bytes.try_into().unwrap(); // SAFETY: We checked for length.
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
                        return Ok(Err(CallError::NotCallable));
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

    let first_version =
        smart_contract_package.next_entity_version_for(protocol_version.value().major);

    let callee_addr = match &caller.context().callee {
        Key::Account(initiator_addr) => initiator_addr.value(),
        Key::SmartContract(smart_contract_addr) => *smart_contract_addr,
        other => panic!("Unexpected callee: {other:?}"),
    };

    let smart_contract_addr: HashAddr = chain_utils::compute_predictable_address(
        caller.context().chain_name.as_bytes(),
        callee_addr,
        bytecode_hash,
        seed,
    );

    let contract_hash =
        chain_utils::compute_next_contract_hash_version(smart_contract_addr, first_version);

    smart_contract_package.insert_entity_version(
        protocol_version.value().major,
        EntityAddr::SmartContract(contract_hash),
    );

    assert!(
        caller
            .context_mut()
            .tracking_copy
            .read(&Key::SmartContract(smart_contract_addr))
            .unwrap()
            .is_none(),
        "TODO: Check if the contract already exists and fail"
    );

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

    let entity_addr = EntityAddr::SmartContract(contract_hash);
    let addressable_entity_key = Key::AddressableEntity(entity_addr);

    // TODO: abort(str) as an alternative to trap
    let address_generator = Arc::clone(&caller.context().address_generator);
    let transaction_hash = caller.context().transaction_hash;
    let main_purse: URef = match system::mint_mint(
        &mut caller.context_mut().tracking_copy,
        transaction_hash,
        address_generator,
        MintArgs {
            initial_balance: U512::zero(),
        },
    ) {
        Ok(uref) => uref,
        Err(mint_error) => {
            error!(?mint_error, "Failed to create a purse");
            return Ok(Err(CallError::CalleeTrapped(
                TrapCode::UnreachableCodeReached,
            )));
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
                .gas_consumed()
                .try_into_remaining()
                .expect("should be remaining");

            let execute_request = ExecuteRequestBuilder::default()
                .with_initiator(caller.context().initiator)
                .with_caller_key(caller.context().callee)
                .with_gas_limit(gas_limit)
                .with_target(ExecutionKind::Stored {
                    address: smart_contract_addr,
                    entry_point: entry_point_name,
                })
                .with_input(input_data.unwrap_or_default())
                .with_transferred_value(value)
                .with_transaction_hash(caller.context().transaction_hash)
                // We're using shared address generator there as we need to preserve and advance the
                // state of deterministic address generator across chain of calls.
                .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
                .with_chain_name(caller.context().chain_name.clone())
                .with_block_time(caller.context().block_time)
                .with_state_hash(Digest::from_raw([0; 32])) // TODO: Carry on state root hash
                .with_block_height(1) // TODO: Carry on block height
                .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32]))) // TODO: Carry on parent block hash
                .build()
                .expect("should build");

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
                        return Ok(Err(host_error));
                    }

                    caller
                        .context_mut()
                        .tracking_copy
                        .apply_changes(effects, cache, messages);

                    output
                }
                Err(ExecuteError::WasmPreparation(_preparation_error)) => {
                    // This is a bug in the EE, as it should have been caught during the preparation
                    // phase when the contract was stored in the global state.
                    todo!()
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

    Ok(Ok(()))
}

#[allow(clippy::too_many_arguments)]
pub fn casper_call<S: GlobalStateReader + 'static, E: Executor + 'static>(
    mut caller: impl Caller<Context = Context<S, E>>,
    address_ptr: u32,
    address_len: u32,
    value_ptr: u32,
    entry_point_ptr: u32,
    entry_point_len: u32,
    input_ptr: u32,
    input_len: u32,
    cb_alloc: u32,
    cb_ctx: u32,
) -> VMResult<HostResult> {
    let call_cost = caller.context().config.host_function_costs().call;
    charge_host_function_call(
        &mut caller,
        &call_cost,
        [
            address_ptr,
            address_len,
            value_ptr,
            entry_point_ptr,
            entry_point_len,
            input_ptr,
            input_len,
            cb_alloc,
            cb_ctx,
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
    let smart_contract_addr: HashAddr = address.try_into().unwrap(); // TODO: Error handling

    let input_data: Bytes = caller.memory_read(input_ptr, input_len as _)?.into();

    let entry_point = {
        let entry_point_bytes = caller.memory_read(entry_point_ptr, entry_point_len as _)?;
        match String::from_utf8(entry_point_bytes) {
            Ok(entry_point) => entry_point,
            Err(utf8_error) => {
                error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                return Ok(Err(CallError::NotCallable));
            }
        }
    };

    let value: u128 = {
        let mut value_bytes = [0u8; 16];
        caller.memory_read_into(value_ptr, &mut value_bytes)?;
        u128::from_le_bytes(value_bytes)
    };

    let tracking_copy = caller.context().tracking_copy.fork2();

    // Take the gas spent so far and use it as a limit for the new VM.
    let gas_limit = caller
        .gas_consumed()
        .try_into_remaining()
        .expect("should be remaining");

    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(caller.context().initiator)
        .with_caller_key(caller.context().callee)
        .with_gas_limit(gas_limit)
        .with_target(ExecutionKind::Stored {
            address: smart_contract_addr,
            entry_point,
        })
        .with_transferred_value(value)
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
        .build()
        .expect("should build");

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
        Err(ExecuteError::WasmPreparation(preparation_error)) => {
            // This is a bug in the EE, as it should have been caught during the preparation phase
            // when the contract was stored in the global state.
            unreachable!("Preparation error: {:?}", preparation_error)
        }
    };

    let gas_spent = gas_usage
        .gas_limit()
        .checked_sub(gas_usage.remaining_points())
        .expect("remaining points always below or equal to the limit");

    caller.consume_gas(gas_spent)?;

    Ok(host_result)
}

pub fn casper_env_caller<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    dest_ptr: u32,
    dest_len: u32,
    entity_kind_ptr: u32,
) -> VMResult<u32> {
    let caller_cost = caller.context().config.host_function_costs().env_caller;
    charge_host_function_call(
        &mut caller,
        &caller_cost,
        [dest_ptr, dest_len, entity_kind_ptr],
    )?;

    // TODO: Decide whether we want to return the full address and entity kind or just the 32 bytes
    // "unified".
    let (entity_kind, data) = match &caller.context().caller {
        Key::Account(account_hash) => (0u32, account_hash.value()),
        Key::SmartContract(smart_contract_addr) => (1u32, *smart_contract_addr),
        other => panic!("Unexpected caller: {other:?}"),
    };
    let mut data = &data[..];
    if dest_ptr == 0 {
        Ok(dest_ptr)
    } else {
        let dest_len = dest_len as usize;
        data = &data[0..cmp::min(32, dest_len)];
        caller.memory_write(dest_ptr, data)?;
        let entity_kind_bytes = entity_kind.to_le_bytes();
        caller.memory_write(entity_kind_ptr, entity_kind_bytes.as_slice())?;
        Ok(dest_ptr + (data.len() as u32))
    }
}

pub fn casper_env_transferred_value<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    output: u32,
) -> Result<(), VMError> {
    let transferred_value_cost = caller
        .context()
        .config
        .host_function_costs()
        .env_transferred_value;
    charge_host_function_call(&mut caller, &transferred_value_cost, [output])?;

    let result = caller.context().transferred_value;
    caller.memory_write(output, &result.to_le_bytes())?;
    Ok(())
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
        [entity_kind, entity_addr_ptr, entity_addr_len, output_ptr],
    )?;

    let entity_key = match EntityKindTag::from_u32(entity_kind) {
        Some(EntityKindTag::Account) => {
            if entity_addr_len != 32 {
                return Ok(0);
            }
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let account_hash: AccountHash = AccountHash::new(entity_addr.try_into().unwrap());

            let account_key = Key::Account(account_hash);
            match caller.context_mut().tracking_copy.read(&account_key) {
                Ok(Some(StoredValue::CLValue(clvalue))) => {

                    let addressible_entity_key = clvalue.into_t::<Key>().expect("should be a key");
                    Either::Right(addressible_entity_key)
                }
                Ok(Some(StoredValue::Account(account))) => {
                    Either::Left(account.main_purse())
                }
                Ok(Some(other_entity)) => {
                    panic!("Unexpected entity type: {other_entity:?}")
                }
                Ok(None) => return Ok(HOST_ERROR_SUCCESS),
                Err(error) => panic!("Error while reading from storage; aborting key={account_key:?} error={error:?}"),
            }
        }
        Some(EntityKindTag::Contract) => {
            if entity_addr_len != 32 {
                return Ok(HOST_ERROR_SUCCESS);
            }
            let hash_bytes = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            let hash_bytes: [u8; 32] = hash_bytes.try_into().unwrap(); // SAFETY: We checked for length.

            let smart_contract_key = Key::SmartContract(hash_bytes);
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressible_entity_hash) => {
                            let key = Key::AddressableEntity(EntityAddr::SmartContract(
                                addressible_entity_hash.value(),
                            ));
                            // Either::Right(Key::AddressableEntity(*addressible_entity_hash))
                            Either::Right(key)
                        }
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressible entity hash for contract"
                            );
                            return Ok(0);
                        }
                    }
                }
                Ok(Some(_)) => {
                    return Ok(0);
                }
                Ok(None) => {
                    // Not found, balance is 0
                    return Ok(0);
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
        None => return Ok(0),
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
        .expect("Total balance");
    assert!(total_balance.value() <= U512::from(u64::MAX));
    let total_balance = total_balance.value().as_u128();

    caller.memory_write(output_ptr, &total_balance.to_le_bytes())?;

    Ok(HOST_ERROR_NOT_FOUND)
}

pub fn casper_transfer<S: GlobalStateReader + 'static, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
    amount_ptr: u32,
) -> VMResult<u32> {
    let transfer_cost = caller.context().config.host_function_costs().transfer;
    charge_host_function_call(
        &mut caller,
        &transfer_cost,
        [entity_addr_ptr, entity_addr_len, amount_ptr],
    )?;

    if entity_addr_len != 32 {
        // Invalid entity address; failing to proceed with the transfer
        return Ok(u32_from_host_result(Err(CallError::NotCallable)));
    }

    let amount: u128 = {
        let mut amount_bytes = [0u8; 16];
        caller.memory_read_into(amount_ptr, &mut amount_bytes)?;
        u128::from_le_bytes(amount_bytes)
    };

    let (target_entity_addr, _runtime_footprint) = {
        let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
        debug_assert_eq!(entity_addr.len(), 32);

        // SAFETY: entity_addr is 32 bytes long
        let account_hash: AccountHash = AccountHash::new(entity_addr.try_into().unwrap());

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

                    indirect.into_t::<Key>().expect("should be key")
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
                        Some(addressible_entity_hash) => Key::AddressableEntity(
                            EntityAddr::SmartContract(addressible_entity_hash.value()),
                        ),
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressible entity hash for contract"
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
        .expect("should read account")
        .expect("should have account");
    let callee_addressable_entity = callee_stored_value
        .into_addressable_entity()
        .expect("should be addressable entity");
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
            panic!("Error while reading from storage; aborting")
        }
    };
    // We don't execute anything as it does not make sense to execute an account as there
    // are no entry points.
    let transaction_hash = caller.context().transaction_hash;
    let address_generator = Arc::clone(&caller.context().address_generator);
    let args = MintTransferArgs {
        source: callee_purse,
        target: target_purse,
        amount: U512::from(amount),
        maybe_to: None,
        id: None,
    };

    let result = system::mint_transfer(
        &mut caller.context_mut().tracking_copy,
        transaction_hash,
        address_generator,
        args,
    );

    Ok(u32_from_host_result(result))
}

pub fn casper_upgrade<S: GlobalStateReader + 'static, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    code_ptr: u32,
    code_size: u32,
    entry_point_ptr: u32,
    entry_point_size: u32,
    input_ptr: u32,
    input_size: u32,
) -> VMResult<HostResult> {
    let upgrade_cost = caller.context().config.host_function_costs().upgrade;
    charge_host_function_call(
        &mut caller,
        &upgrade_cost,
        [
            code_ptr,
            code_size,
            entry_point_ptr,
            entry_point_size,
            input_ptr,
            input_size,
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
                    return Ok(Err(CallError::NotCallable));
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
            return Ok(Err(CallError::NotCallable));
        }
        addressable_entity_key @ Key::SmartContract(smart_contract_addr) => {
            let smart_contract_key = addressable_entity_key;
            match caller.context_mut().tracking_copy.read(&smart_contract_key) {
                Ok(Some(StoredValue::SmartContract(smart_contract_package))) => {
                    match smart_contract_package.versions().latest() {
                        Some(addressible_entity_hash) => {
                            let key = Key::AddressableEntity(EntityAddr::SmartContract(
                                addressible_entity_hash.value(),
                            ));
                            (smart_contract_addr, key)
                        }
                        None => {
                            warn!(
                                ?smart_contract_key,
                                "Unable to find latest addressible entity hash for contract"
                            );
                            return Ok(Err(CallError::NotCallable));
                        }
                    }
                }
                Ok(Some(other)) => panic!("should be smart contract but got {other:?}"),
                Ok(None) => return Ok(Err(CallError::NotCallable)),
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
        Ok(None) => return Ok(Err(CallError::NotCallable)),
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
            .gas_consumed()
            .try_into_remaining()
            .expect("should be remaining");

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
            .build()
            .expect("should build");

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
                    return Ok(Err(host_error));
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
            Err(ExecuteError::WasmPreparation(preparation_error)) => {
                // Unable to call contract because the wasm is broken.
                error!(
                    ?preparation_error,
                    "Wasm preparation error while performing upgrade"
                );
                return Ok(Err(CallError::NotCallable));
            }
        }
    }

    Ok(Ok(()))
}

// TODO: Should this be blocktime instead of block_time? [Consistency]
// If addressed, make sure to fix this in chainspec too
pub fn casper_env_block_time<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
) -> VMResult<u64> {
    let block_time_cost = caller.context().config.host_function_costs().env_block_time;
    charge_host_function_call(&mut caller, &block_time_cost, [])?;

    let block_time = caller.context().block_time;
    Ok(block_time.value())
}

pub fn casper_emit<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    topic_name_ptr: u32,
    topic_name_size: u32,
    payload_ptr: u32,
    payload_size: u32,
) -> VMResult<u32> {
    // Charge for parameter weights.
    let emit_host_function = caller.context().config.host_function_costs().emit;

    charge_host_function_call(
        &mut caller,
        &emit_host_function,
        [topic_name_ptr, topic_name_size, payload_ptr, payload_size],
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
            // We're lazily creating message topics and this operation is idempotent. Therefore
            // already existing topic is not an issue.
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
                        .unwrap()
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
                CLValue::into_t(value_pair).expect("Tuple");
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
    let message_value = StoredValue::Message(message.checksum().expect("Checksum"));
    let message_count_pair: MessageCountPair = (current_block_time, block_message_count);
    let block_message_count_value = StoredValue::CLValue(
        CLValue::from_t(message_count_pair).expect("Serialize block message pair"),
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
