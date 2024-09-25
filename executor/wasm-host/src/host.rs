use std::{borrow::Cow, cmp, num::NonZeroU32, sync::Arc};

use crate::system::{self, MintArgs, MintTransferArgs};

use crate::{abi::CreateResult, context::Context};
use bytes::Bytes;
use casper_executor_wasm_common::{
    entry_point::{
        ENTRY_POINT_PAYMENT_CALLER, ENTRY_POINT_PAYMENT_SELF_ONLY, ENTRY_POINT_PAYMENT_SELF_ONWARD,
    },
    error::{HOST_ERROR_INVALID_DATA, HOST_ERROR_INVALID_INPUT, HOST_ERROR_NOT_FOUND},
    flags::ReturnFlags,
    keyspace::{Keyspace, KeyspaceTag},
};
use casper_executor_wasm_interface::u32_from_host_result;
use casper_storage::{global_state::GlobalStateReader, tracking_copy::TrackingCopyExt};
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, MessageTopics, NamedKeyAddr},
    AddressableEntity, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, CLType, Digest,
    EntityAddr, EntityKind, EntryPoint, EntryPointAccess, EntryPointAddr, EntryPointPayment,
    EntryPointType, EntryPointValue, Groups, HashAddr, Key, Package, PackageHash, PackageStatus,
    ProtocolVersion, StoredValue, TransactionRuntime, URef, U512,
};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use tracing::{error, info};

use casper_executor_wasm_interface::{
    executor::{ExecuteError, ExecuteRequestBuilder, ExecuteResult, ExecutionKind, Executor},
    Caller, HostError, HostResult, MeteringPoints, TrapCode, VMError, VMResult,
};

use crate::abi::ReadInfo;

#[derive(Debug, Copy, Clone, FromPrimitive, PartialEq)]
enum EntityKindTag {
    Account = 0,
    Contract = 1,
}

/// Write value under a key.
pub fn casper_write<S: GlobalStateReader, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
    value_ptr: u32,
    value_size: u32,
) -> VMResult<i32> {
    let keyspace_tag = match KeyspaceTag::from_u64(key_space) {
        Some(keyspace_tag) => keyspace_tag,
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
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
                    return Ok(1);
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
                return Ok(1);
            }

            Keyspace::PaymentInfo(key_name)
        }
    };

    let global_state_key = match keyspace_to_global_state_key(caller.context(), keyspace) {
        Some(global_state_key) => global_state_key,
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
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
                [ENTRY_POINT_PAYMENT_SELF_ONLY] => EntryPointPayment::SelfOnly,
                [ENTRY_POINT_PAYMENT_SELF_ONWARD] => EntryPointPayment::SelfOnward,
                _ => {
                    // Invalid entry point payment variant
                    return Ok(HOST_ERROR_INVALID_INPUT);
                }
            };

            let entry_point = EntryPoint::new(
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

    caller
        .context_mut()
        .tracking_copy
        .write(global_state_key, stored_value);

    Ok(0)
}

pub fn casper_print<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<Context = Context<S, E>>,
    message_ptr: u32,
    message_size: u32,
) -> VMResult<()> {
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
) -> Result<i32, VMError> {
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
            return Ok(1);
        }
    };
    let global_state_read_result = caller.context_mut().tracking_copy.read(&global_state_key);

    let global_state_raw_bytes: Cow<[u8]> = match global_state_read_result {
        Ok(Some(StoredValue::RawBytes(raw_bytes))) => Cow::Owned(raw_bytes),
        Ok(Some(StoredValue::EntryPoint(EntryPointValue::V1CasperVm(entry_point)))) => {
            match entry_point.entry_point_payment() {
                EntryPointPayment::Caller => Cow::Borrowed(&[ENTRY_POINT_PAYMENT_CALLER]),
                EntryPointPayment::SelfOnly => Cow::Borrowed(&[ENTRY_POINT_PAYMENT_SELF_ONLY]),
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
    let entity_addr = match context.callee {
        Key::Account(account_hash) => EntityAddr::new_account(account_hash.value()),
        Key::AddressableEntity(addressable_entity_hash) => {
            EntityAddr::new_smart_contract(addressable_entity_hash.value())
        }
        _ => {
            // This should never happen, as the caller is always an account or a smart contract.
            return None;
        }
    };

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

    if out_ptr == 0 {
        Ok(out_ptr)
    } else {
        caller.memory_write(out_ptr, &input)?;
        Ok(out_ptr + (input.len() as u32))
    }
}

/// Returns from the execution of a smart contract with an optional flags.
pub fn casper_return<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<Context = Context<S, E>>,
    flags: u32,
    data_ptr: u32,
    data_len: u32,
) -> VMResult<()> {
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
    result_ptr: u32,
) -> VMResult<HostResult> {
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
                        return Ok(Err(HostError::NotCallable));
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

    // let manifest = caller.memory_read(manifest_ptr, mem::size_of::<Manifest>())?;
    // let bytes = manifest.as_slice();

    // let manifest = match safe_transmute::transmute_one::<Manifest>(bytes) {
    //     Ok(manifest) => manifest,
    //     Err(error) => {
    //         todo!("handle error {:?}", error);
    //     }
    // // };

    // let entry_points_bytes = caller.memory_read(
    //     manifest.entry_points_ptr,
    //     (manifest.entry_points_size as usize) * mem::size_of::<EntryPoint>(),
    // )?;

    // let entry_points =
    //     match safe_transmute::transmute_many::<EntryPoint, SingleManyGuard>(&entry_points_bytes)
    // {         Ok(entry_points) => entry_points,
    //         Err(error) => todo!("handle error {:?}", error),
    //     };

    // 1. Store package hash
    let contract_package = Package::new(
        Default::default(),
        Default::default(),
        Groups::default(),
        PackageStatus::Unlocked,
    );

    let bytecode_hash = Digest::hash(&code);

    // TODO: Compute predictable address based on the callee (as the owner) and include a seed.
    // let contract_hash: HashAddr = {
    //     let bytecode_hash = Digest::hash(&code);
    //     let contract_hash = chain_utils::compute_predictable_address(
    //         caller.context().chain_name.as_bytes(),
    //         caller.context().initiator,
    //         bytecode_hash.into(),
    //     );
    //     contract_hash
    // };

    let package_hash_bytes: HashAddr;
    let contract_hash: HashAddr;

    {
        let mut gen = caller.context().address_generator.write();
        package_hash_bytes = gen.create_address();
        contract_hash = gen.create_address();
    }

    caller.context_mut().tracking_copy.write(
        Key::Package(package_hash_bytes),
        StoredValue::Package(contract_package),
    );

    // 2. Store wasm

    let bytecode = ByteCode::new(ByteCodeKind::V2CasperWasm, code.clone().into());
    let bytecode_addr = ByteCodeAddr::V2CasperWasm(bytecode_hash.value());

    caller.context_mut().tracking_copy.write(
        Key::ByteCode(bytecode_addr),
        StoredValue::ByteCode(bytecode),
    );

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
            return Ok(Err(HostError::CalleeTrapped(
                TrapCode::UnreachableCodeReached,
            )));
        }
    };

    let addressable_entity = AddressableEntity::new(
        PackageHash::new(package_hash_bytes),
        ByteCodeHash::new(bytecode_hash.value()),
        ProtocolVersion::V2_0_0,
        main_purse,
        AssociatedKeys::default(),
        ActionThresholds::default(),
        MessageTopics::default(),
        EntityKind::SmartContract(TransactionRuntime::VmCasperV2),
    );

    caller.context_mut().tracking_copy.write(
        addressable_entity_key,
        StoredValue::AddressableEntity(addressable_entity),
    );

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
                .with_callee_key(addressable_entity_key)
                .with_gas_limit(gas_limit)
                .with_target(ExecutionKind::Stored {
                    address: entity_addr,
                    entry_point: entry_point_name, //Some(Selector::new(selector)),
                })
                .with_input(input_data.unwrap_or_default())
                .with_transferred_value(value)
                .with_transaction_hash(caller.context().transaction_hash)
                // We're using shared address generator there as we need to preserve and advance the
                // state of deterministic address generator across chain of calls.
                .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
                .with_chain_name(caller.context().chain_name.clone())
                .with_block_time(caller.context().block_time)
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
                }) => {
                    // output

                    caller.consume_gas(gas_usage.gas_spent());

                    if let Some(host_error) = host_error {
                        return Ok(Err(host_error));
                    }

                    caller
                        .context_mut()
                        .tracking_copy
                        .apply_changes(effects, cache);

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

    // if let Some(state) = initial_state {
    //     eprintln!(
    //         "Write initial state {}bytes under {contract_hash:?}",
    //         state.len()
    //     );
    //     let smart_contract_addr = EntityAddr::SmartContract(contract_hash);
    //     let key = Key::State(smart_contract_addr);
    //     caller
    //         .context_mut()
    //         .tracking_copy
    //         .write(key, StoredValue::RawBytes(state.into()));
    // }

    let create_result = CreateResult {
        package_address: package_hash_bytes,
        contract_address: contract_hash,
        version: 1,
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
    let address: HashAddr = address.try_into().unwrap(); // TODO: Error handling

    let input_data: Bytes = caller.memory_read(input_ptr, input_len as _)?.into();

    let entry_point = {
        let entry_point_bytes = caller.memory_read(entry_point_ptr, entry_point_len as _)?;
        match String::from_utf8(entry_point_bytes) {
            Ok(entry_point) => entry_point,
            Err(utf8_error) => {
                error!(%utf8_error, "entry point name is not a valid utf-8 string; unable to call");
                return Ok(Err(HostError::NotCallable));
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

    let entity_addr = EntityAddr::new_smart_contract(address);
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(caller.context().initiator)
        .with_caller_key(caller.context().callee)
        .with_callee_key(Key::AddressableEntity(entity_addr))
        .with_gas_limit(gas_limit)
        .with_target(ExecutionKind::Stored {
            address: entity_addr,
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
                        .apply_changes(effects, cache);
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

    match caller.consume_gas(gas_spent) {
        MeteringPoints::Remaining(_) => {}
        MeteringPoints::Exhausted => {
            todo!("exhausted")
        }
    }

    Ok(host_result)
}

pub fn casper_env_caller<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<Context = Context<S, E>>,
    dest_ptr: u32,
    dest_len: u32,
    entity_kind_ptr: u32,
) -> VMResult<u32> {
    // TODO: Decide whether we want to return the full address and entity kind or just the 32 bytes
    // "unified".
    let (entity_kind, data) = match &caller.context().caller {
        Key::Account(account_hash) => (0u32, account_hash.value()),
        Key::AddressableEntity(entity_addr) => (1u32, entity_addr.value()),
        other => panic!("Unexpected caller: {:?}", other),
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
    caller: impl Caller<Context = Context<S, E>>,
    output: u32,
) -> Result<(), VMError> {
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

                    clvalue.into_t::<Key>().expect("should be a key")
                }
                Ok(Some(other_entity)) => {
                    panic!("Unexpected entity type: {:?}", other_entity)
                }
                Ok(None) => return Ok(0),
                Err(error) => panic!("Error while reading from storage; aborting key={account_key:?} error={error:?}"),
            }
        }
        Some(EntityKindTag::Contract) => {
            if entity_addr_len != 32 {
                return Ok(0);
            }
            let hash_bytes = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            Key::AddressableEntity(EntityAddr::SmartContract(hash_bytes.try_into().unwrap()))
        }
        None => return Ok(0),
    };

    let purse = match caller.context_mut().tracking_copy.read(&entity_key) {
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
            addressable_entity.main_purse()
        }
        Ok(Some(other_entity)) => {
            panic!("Unexpected entity type: {:?}", other_entity)
        }
        Ok(None) => return Ok(0),
        Err(error) => {
            panic!("Error while reading from storage; aborting key={entity_key:?} error={error:?}")
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

    Ok(1)
}

pub fn casper_transfer<S: GlobalStateReader + 'static, E: Executor>(
    mut caller: impl Caller<Context = Context<S, E>>,
    entity_kind: u32,
    entity_addr_ptr: u32,
    entity_addr_len: u32,
    amount_ptr: u32,
) -> VMResult<u32> {
    if entity_addr_len != 32 {
        // Invalid entity address; failing to proceed with the transfer
        return Ok(u32_from_host_result(Err(HostError::NotCallable)));
    }

    let entity_kind = match EntityKindTag::from_u32(entity_kind) {
        Some(entity_kind) => entity_kind,
        None => {
            // Unknown target entity kind; failing to proceed with the transfer
            return Ok(u32_from_host_result(Err(HostError::NotCallable))); // fail
        }
    };

    let amount: u128 = {
        let mut amount_bytes = [0u8; 16];
        caller.memory_read_into(amount_ptr, &mut amount_bytes)?;
        u128::from_le_bytes(amount_bytes)
    };

    let target_entity_addr = match entity_kind {
        EntityKindTag::Account => {
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            debug_assert_eq!(entity_addr.len(), 32);

            // SAFETY: entity_addr is 32 bytes long
            let account_hash: AccountHash = AccountHash::new(entity_addr.try_into().unwrap());

            // Resolve indirection to get to the actual addressable entity
            let account_key = Key::Account(account_hash);
            match caller.context_mut().tracking_copy.read(&account_key) {
                Ok(Some(StoredValue::CLValue(indirect))) => {
                    // is it an account?
                    let addressable_entity_key = indirect.into_t::<Key>().expect("should be key");
                    addressable_entity_key
                        .into_entity_addr()
                        .expect("should be entity addr")
                }
                Ok(Some(other)) => panic!("should be cl value but got {other:?}"),
                Ok(None) => return Ok(u32_from_host_result(Err(HostError::NotCallable))),
                Err(error) => {
                    error!(
                        ?error,
                        ?account_key,
                        "Error while reading from storage; aborting"
                    );
                    panic!("Error while reading from storage")
                }
            }
        }
        EntityKindTag::Contract => {
            let entity_addr = caller.memory_read(entity_addr_ptr, entity_addr_len as usize)?;
            debug_assert_eq!(entity_addr.len(), 32);

            // SAFETY: entity_addr is 32 bytes long
            let entity_addr: HashAddr = entity_addr.try_into().unwrap();
            EntityAddr::SmartContract(entity_addr)
        }
    };

    let callee_addressable_entity_key = match caller.context().callee {
        callee_account_key @ Key::Account(_account_hash) => {
            match caller.context_mut().tracking_copy.read(&callee_account_key) {
                Ok(Some(StoredValue::CLValue(indirect))) => {
                    // is it an account?

                    indirect.into_t::<Key>().expect("should be key")
                }
                Ok(Some(other)) => panic!("should be cl value but got {other:?}"),
                Ok(None) => return Ok(u32_from_host_result(Err(HostError::NotCallable))),
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
        addressable_entity_key @ Key::AddressableEntity(_entity_addr) => addressable_entity_key,
        other => panic!("should be account or addressable entity but got {other:?}"),
    };

    let _callee_entity_addr = callee_addressable_entity_key
        .into_entity_addr()
        .expect("should be entity addr");

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

    match entity_kind {
        EntityKindTag::Account => {
            let target_purse = match caller
                .context_mut()
                .tracking_copy
                .read(&Key::AddressableEntity(target_entity_addr))
            {
                Ok(Some(StoredValue::Account(account))) => {
                    panic!("Expected AddressableEntity but got {:?}", account)
                }
                Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
                    addressable_entity.main_purse()
                }
                Ok(Some(other_entity)) => {
                    panic!("Unexpected entity type: {:?}", other_entity)
                }
                Ok(None) => {
                    panic!("Addressable entity not found for key={target_entity_addr:?}");
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
        EntityKindTag::Contract => {
            // let callee_purse = callee_stored_value.main_purse();

            let transaction_hash = caller.context().transaction_hash;
            let _address_generator = Arc::clone(&caller.context().address_generator);

            let tracking_copy = caller.context().tracking_copy.fork2();

            // Take the gas spent so far and use it as a limit for the new VM.
            let gas_limit = caller
                .gas_consumed()
                .try_into_remaining()
                .expect("should be remaining");

            let address_generator = Arc::clone(&caller.context().address_generator);
            let execute_request = ExecuteRequestBuilder::default()
                .with_initiator(caller.context().initiator)
                .with_caller_key(caller.context().callee)
                // .with_callee_key(Key::AddressableEntity(EntityAddr::new_smart_contract(
                //     address,
                // )))
                .with_callee_key(Key::AddressableEntity(target_entity_addr))
                .with_gas_limit(gas_limit)
                .with_target(ExecutionKind::Stored {
                    address: target_entity_addr,
                    entry_point: "__casper_fallback".to_string(),
                })
                .with_transferred_value(amount)
                .with_input(Bytes::new())
                .with_transaction_hash(transaction_hash)
                // We're using shared address generator there as we need to preserve and advance the
                // state of deterministic address generator across chain of calls.
                .with_shared_address_generator(address_generator)
                .with_chain_name(caller.context().chain_name.clone())
                .with_block_time(caller.context().block_time)
                .build()
                .expect("should build");

            match caller
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
                }) => {
                    caller.consume_gas(gas_usage.gas_spent());
                    let _ = output; // TODO: What to do with the output of a fallback code? Need to emit a message
                                    // with this

                    let host_result = match host_error {
                        Some(host_error) => Err(host_error),
                        None => {
                            caller
                                .context_mut()
                                .tracking_copy
                                .apply_changes(effects, cache);
                            Ok(())
                        }
                    };

                    let gas_spent = gas_usage
                        .gas_limit()
                        .checked_sub(gas_usage.remaining_points())
                        .expect("remaining points always below or equal to the limit");

                    match caller.consume_gas(gas_spent) {
                        MeteringPoints::Remaining(_) => {}
                        MeteringPoints::Exhausted => {
                            todo!("exhausted")
                        }
                    }

                    Ok(u32_from_host_result(host_result))
                }
                Err(ExecuteError::WasmPreparation(preparation_error)) => {
                    // This is a bug in the EE, as it should have been caught during the preparation
                    // phase when the contract was stored in the global state.
                    unreachable!("Preparation error: {:?}", preparation_error)
                }
            }
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
) -> VMResult<HostResult> {
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
                    return Ok(Err(HostError::NotCallable));
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

    let callee_addressable_entity_key = match caller.context().callee {
        Key::Account(_account_hash) => {
            error!("Account upgrade is not possible");
            return Ok(Err(HostError::NotCallable));
        }
        addressable_entity_key @ Key::AddressableEntity(_entity_addr) => addressable_entity_key,
        other => panic!("should be account or addressable entity but got {other:?}"),
    };

    let callee_addressable_entity = match caller
        .context_mut()
        .tracking_copy
        .read(&callee_addressable_entity_key)
    {
        Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => addressable_entity,
        Ok(Some(other_entity)) => {
            panic!("Unexpected entity type: {:?}", other_entity)
        }
        Ok(None) => return Ok(Err(HostError::NotCallable)),
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
    caller.context_mut().tracking_copy.write(
        bytecode_key,
        StoredValue::ByteCode(ByteCode::new(
            ByteCodeKind::V2CasperWasm,
            code.clone().into(),
        )),
    );

    // 3. Execute upgrade routine (if specified)
    // this code should handle reading old state, and saving new state

    if let Some(entry_point_name) = entry_point {
        // Take the gas spent so far and use it as a limit for the new VM.
        let gas_limit = caller
            .gas_consumed()
            .try_into_remaining()
            .expect("should be remaining");

        let entity_addr = callee_addressable_entity_key
            .into_entity_addr()
            .expect("should be entity addr");

        let execute_request = ExecuteRequestBuilder::default()
            .with_initiator(caller.context().initiator)
            .with_caller_key(caller.context().callee)
            .with_callee_key(callee_addressable_entity_key)
            .with_gas_limit(gas_limit)
            .with_target(ExecutionKind::Stored {
                address: entity_addr,
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
            }) => {
                // output

                caller.consume_gas(gas_usage.gas_spent());

                if let Some(host_error) = host_error {
                    return Ok(Err(host_error));
                }

                caller
                    .context_mut()
                    .tracking_copy
                    .apply_changes(effects, cache);

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
                return Ok(Err(HostError::NotCallable));
            }
        }
    }

    Ok(Ok(()))
}

pub fn casper_env_block_time<S: GlobalStateReader, E: Executor>(
    caller: impl Caller<Context = Context<S, E>>,
) -> VMResult<u64> {
    let block_time = caller.context().block_time;
    Ok(block_time.millis())
}
