//! Implementation of all host functions.
pub(crate) mod abi;

use bytes::Bytes;
use casper_storage::global_state::{self, state::StateReader};
use casper_types::{
    addressable_entity::NamedKeyAddr,
    contracts::{ContractManifest, ContractV2, EntryPointV2},
    package::PackageStatus,
    ByteCode, ByteCodeAddr, ByteCodeKind, Digest, EntityAddr, EntityVersions, Groups, Key, Package,
    StoredValue, URef,
};
use rand::Rng;
use safe_transmute::SingleManyGuard;
use std::{cmp, collections::BTreeSet, mem, num::NonZeroU32};
use tracing::error;
use vm_common::{
    flags::{EntryPointFlags, ReturnFlags},
    keyspace::Keyspace,
};

use crate::{
    backend::{Caller, Context, MeteringPoints, WasmInstance},
    host::abi::{CreateResult, EntryPoint, Manifest},
    storage::{self, Address},
    ConfigBuilder, VMError, VMResult, VM,
};

use self::abi::ReadInfo;

/// Represents the result of a host function call.
///
/// 0 is used as a success.
#[derive(Debug)]
pub enum HostError {
    /// Callee contract reverted.
    CalleeReverted = 1,
    /// Called contract trapped.
    CalleeTrapped = 2,
    /// Called contract reached gas limit.
    CalleeGasDepleted = 3,
}

type HostResult = Result<(), HostError>;

pub(crate) fn u32_from_host_result(result: HostResult) -> u32 {
    match result {
        Ok(()) => 0,
        Err(error) => error as u32,
    }
}

/// Write value under a key.
pub(crate) fn casper_write<S: StateReader<Key, StoredValue, Error = global_state::error::Error>>(
    mut caller: impl Caller<S>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
    value_ptr: u32,
    value_size: u32,
) -> VMResult<i32> {
    let key = caller.memory_read(key_ptr, key_size.try_into().unwrap())?;

    let global_state_key = match Keyspace::from_raw_parts(key_space, &key) {
        Some(keyspace) => keyspace_to_global_state_key(caller.context().address, keyspace),
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
        }
    };

    let value = caller.memory_read(value_ptr, value_size.try_into().unwrap())?;

    caller
        .context_mut()
        .storage
        .write(global_state_key, StoredValue::RawBytes(value));

    Ok(0)
}

pub(crate) fn casper_print<S: StateReader<Key, StoredValue, Error = global_state::error::Error>>(
    caller: impl Caller<S>,
    message_ptr: u32,
    message_size: u32,
) -> VMResult<()> {
    let vec = caller.memory_read(message_ptr, message_size.try_into().unwrap())?;
    let msg = String::from_utf8_lossy(&vec);
    eprintln!("⛓️ {msg}");
    Ok(())
}

/// Write value under a key.
pub(crate) fn casper_read<S: StateReader<Key, StoredValue, Error = global_state::error::Error>>(
    mut caller: impl Caller<S>,
    key_tag: u64,
    key_ptr: u32,
    key_size: u32,
    info_ptr: u32,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> Result<i32, VMError> {
    // TODO: Opportunity for optimization: don't read data under key_ptr if given key space does not require it.
    let key = caller.memory_read(key_ptr, key_size.try_into().unwrap())?;

    let keyspace = match Keyspace::from_raw_parts(key_tag, &key) {
        Some(keyspace) => keyspace,
        None => {
            // Unknown keyspace received, return error
            return Ok(1);
        }
    };

    // Convert keyspace to a global state `Key`
    let global_state_key = keyspace_to_global_state_key(caller.context().address, keyspace);

    match caller.context_mut().storage.read(&global_state_key) {
        Ok(Some(StoredValue::RawBytes(raw_bytes))) => {
            let out_ptr: u32 = if cb_alloc != 0 {
                caller.alloc(cb_alloc, raw_bytes.len(), alloc_ctx)?
            } else {
                // treats alloc_ctx as data
                alloc_ctx
            };

            let read_info = ReadInfo {
                data: out_ptr,
                data_size: raw_bytes.len().try_into().unwrap(),
            };

            let read_info_bytes = safe_transmute::transmute_one_to_bytes(&read_info);
            caller.memory_write(info_ptr, read_info_bytes)?;
            if out_ptr != 0 {
                caller.memory_write(out_ptr, &raw_bytes)?;
            }
            Ok(0)
        }
        Ok(Some(stored_value)) => {
            // TODO: Backwards compatibility with old EE, although it's not clear if we should do it at the storage level.
            // Since new VM has storage isolated from the Wasm (i.e. we have Keyspace on the wasm which gets converted to a global state `Key`).
            // I think if we were to pursue this we'd add a new `Keyspace` enum variant for each old VM supported Key types (i.e. URef, Dictionary perhaps) for some period of time, then deprecate this.
            todo!("Unsupported {stored_value:?}")
        }
        Ok(None) => Ok(1), // Entry does not exists
        Err(error) => {
            // To protect the network against potential non-determinism (i.e. one validator runs out of space or just faces I/O issues that other validators may not have) we're simply aborting the process, hoping that once the node goes back online issues are resolved on the validator side.
            // TODO: We should signal this to the contract runtime somehow, and let validator nodes skip execution.
            error!(?error, "Error while reading from storage; aborting");
            panic!("Error while reading from storage; aborting key={global_state_key:?} error={error:?}")
        }
    }
}

fn keyspace_to_global_state_key<'a>(address: [u8; 32], keyspace: Keyspace<'a>) -> Key {
    match keyspace {
        Keyspace::State => casper_types::Key::State(EntityAddr::SmartContract(address)),
        Keyspace::Context(payload) => {
            let digest = Digest::hash(payload);
            casper_types::Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                EntityAddr::SmartContract(address),
                digest.value(),
            ))
        }
    }
}

pub(crate) fn casper_copy_input<
    S: StateReader<Key, StoredValue, Error = global_state::error::Error>,
>(
    mut caller: impl Caller<S>,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    let input = caller.config().input.clone();

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
pub(crate) fn casper_return<
    S: StateReader<Key, StoredValue, Error = global_state::error::Error>,
>(
    caller: impl Caller<S>,
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

pub(crate) fn casper_create_contract<
    S: StateReader<Key, StoredValue, Error = global_state::error::Error> + 'static,
>(
    mut caller: impl Caller<S>,
    code_ptr: u32,
    code_len: u32,
    manifest_ptr: u32,
    selector: u32,
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

    // For calling a constructor
    let constructor_selector = NonZeroU32::new(selector);

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> = if input_ptr == 0 {
        None
    } else {
        let input_data = caller.memory_read(input_ptr, input_len as _)?.into();
        Some(input_data)
    };

    let manifest = caller.memory_read(manifest_ptr, mem::size_of::<Manifest>())?;
    let bytes = manifest.as_slice();

    let manifest = match safe_transmute::transmute_one::<Manifest>(bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            todo!("handle error {:?}", error);
        }
    };

    let entry_points_bytes = caller.memory_read(
        manifest.entry_points_ptr,
        (manifest.entry_points_size as usize) * mem::size_of::<EntryPoint>(),
    )?;

    let entry_points =
        match safe_transmute::transmute_many::<EntryPoint, SingleManyGuard>(&entry_points_bytes) {
            Ok(entry_points) => entry_points,
            Err(error) => todo!("handle error {:?}", error),
        };

    let entrypoints = {
        let mut vec = Vec::new();

        for entry_point in entry_points {
            vec.push(storage::EntryPoint {
                selector: entry_point.selector,
                function_index: entry_point.fptr,
                flags: EntryPointFlags::from_bits_truncate(entry_point.flags),
            })
        }

        vec
    };

    // 1. Store package hash
    let access_key = URef::default();

    let contract_package = Package::new(
        access_key,
        EntityVersions::new(),
        BTreeSet::new(),
        Groups::new(),
        PackageStatus::Unlocked,
    );

    let mut rng = rand::thread_rng(); // TODO: Deterministic random number generator
    let package_hash: Address = rng.gen(); // TODO: Do we need packages at all in new VM?
    let contract_hash: Address = rng.gen();

    caller.context_mut().storage.write(
        Key::Package(package_hash),
        StoredValue::Package(contract_package),
    );

    // 2. Store wasm
    let wasm_hash = Digest::hash(&code);

    let bytecode = ByteCode::new(ByteCodeKind::V1CasperWasm, code.clone().into());
    let bytecode_addr = ByteCodeAddr::V1CasperWasm(wasm_hash.value());

    caller.context_mut().storage.write(
        Key::ByteCode(bytecode_addr),
        StoredValue::ByteCode(bytecode),
    );

    // 3. Store addressable entity

    let key = Key::AddressableEntity(EntityAddr::SmartContract(contract_hash));

    let owner = EntityAddr::SmartContract(caller.context().address);

    let entry_points = entrypoints
        .iter()
        .map(|entrypoint| {
            let entry_point = EntryPointV2 {
                selector: entrypoint.selector,
                function_index: entrypoint.function_index,
                flags: entrypoint.flags.bits(),
            };
            entry_point
        })
        .collect();

    let contract = ContractV2::V2(ContractManifest {
        owner: caller.context().address,
        bytecode_addr,
        entry_points,
    });

    caller
        .context_mut()
        .storage
        .write(key, StoredValue::ContractV2(contract));

    let initial_state = if let Some(constructor_selector) = constructor_selector {
        // Find all entrypoints with a flag set to CONSTRUCTOR
        let constructors: Vec<_> = entrypoints
            .iter()
            .filter_map(|entrypoint| {
                if entrypoint.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                    Some(entrypoint)
                } else {
                    None
                }
            })
            .filter(|entry_point| entry_point.selector == constructor_selector.get())
            .collect();

        // TODO: Should we validate amount of constructors or just rely on the fact that the proc
        // macro will statically check it, and the document the behavior that only first
        // constructor will be called

        match constructors.first() {
            Some(first_constructor) => {
                let mut vm = VM::new();

                let storage = caller.context().storage.fork2();

                let current_config = caller.config().clone();
                let gas_limit = caller
                    .gas_consumed()
                    .try_into_remaining()
                    .expect("should be remaining");

                let new_context = Context {
                    address: contract_hash,
                    storage,
                };

                let new_config = ConfigBuilder::new()
                    .with_gas_limit(gas_limit)
                    .with_memory_limit(current_config.memory_limit)
                    .with_input(input_data.unwrap_or(Bytes::new()))
                    .build();

                let mut new_instance = vm
                    .prepare(code.clone(), new_context, new_config)
                    .expect("should prepare instance");

                let (call_result, gas_usage) =
                    new_instance.call_function(first_constructor.function_index);

                let (return_data, host_result) = match call_result {
                    Ok(()) => {
                        // Contract did not call `casper_return` - implies that it did not revert,
                        // and did not pass any data.
                        (None, Ok(()))
                    }
                    Err(VMError::Return { flags, data }) => {
                        // If the contract called `casper_return` function with revert bit set, we
                        // have to pass the reverted code to the caller.
                        let host_result = if flags.contains(ReturnFlags::REVERT) {
                            Err(HostError::CalleeReverted)
                        } else {
                            Ok(())
                        };
                        (data, host_result)
                    }
                    Err(VMError::OutOfGas) => {
                        todo!()
                    }
                    Err(VMError::Trap(_trap)) => (None, Err(HostError::CalleeTrapped)),
                    Err(other_error) => todo!("{other_error:?}"),
                };

                // Calculate how much gas was spent during execution of the called contract.
                let gas_spent = gas_usage
                    .gas_limit
                    .checked_sub(gas_usage.remaining_points)
                    .expect("remaining points always below or equal to the limit");

                match caller.consume_gas(gas_spent) {
                    MeteringPoints::Remaining(_) => {}
                    MeteringPoints::Exhausted => {
                        todo!("exhausted")
                    }
                }

                let return_data = match host_result {
                    Ok(()) => return_data,
                    error @ Err(_) => {
                        // If the constructor failed, we have to return the error to the caller.
                        return Ok(error);
                    }
                };

                let new_context = new_instance.teardown();

                // Constructor was successful, merge all effects into the caller.
                caller.context_mut().storage.merge(new_context.storage);

                return_data
            }
            None => {
                todo!(
                    "Constructor with selector {:?} not found; available constructors {:?}",
                    constructor_selector,
                    constructors
                )
            }
        }
    } else {
        None
    };

    if let Some(state) = initial_state {
        eprintln!(
            "Write initial state {}bytes under {contract_hash:?}",
            state.len()
        );
        let smart_contract_addr = EntityAddr::SmartContract(contract_hash);
        let key = Key::State(smart_contract_addr);
        caller
            .context_mut()
            .storage
            .write(key, StoredValue::RawBytes(state.into()));
    }

    let create_result = CreateResult {
        package_address: package_hash,
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
pub(crate) fn casper_call<
    S: StateReader<Key, StoredValue, Error = global_state::error::Error> + 'static,
>(
    mut caller: impl Caller<S>,
    address_ptr: u32,
    address_len: u32,
    value: u64,
    selector: u32,
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
    let address: Address = address.try_into().unwrap(); // TODO: Error handling
    let input_data: Bytes = caller.memory_read(input_ptr, input_len as _)?.into();

    let key = Key::AddressableEntity(EntityAddr::SmartContract(address)); // TODO: Error handling
    let contract = caller
        .context_mut()
        .storage
        .read(&key)
        .expect("should read contract");

    match contract {
        Some(StoredValue::ContractV2(ContractV2::V2(manifest))) => {
            let wasm_key = match manifest.bytecode_addr {
                ByteCodeAddr::V1CasperWasm(wasm_hash) => {
                    Key::ByteCode(ByteCodeAddr::V1CasperWasm(wasm_hash))
                }
                ByteCodeAddr::Empty => {
                    // Note: What should happen?
                    todo!("empty bytecode addr")
                }
            };

            // Note: Bytecode stored in the globalstate has a "kind" option - currently we know we have a v2 bytecode as the stored contract is of "V2" variant.
            let wasm_bytes = caller
                .context_mut()
                .storage
                .read(&wasm_key)
                .expect("should read wasm")
                .expect("should have wasm bytes")
                .into_byte_code()
                .expect("should be byte code")
                .take_bytes();

            let mut vm = VM::new();

            let storage = caller.context().storage.fork2();

            let new_context = Context {
                address: address.try_into().unwrap(),
                storage,
            };

            let current_config = caller.config().clone();

            // Take the gas spent so far and use it as a limit for the new VM.
            let gas_limit = caller
                .gas_consumed()
                .try_into_remaining()
                .expect("should be remaining");

            let new_config = ConfigBuilder::new()
                .with_gas_limit(gas_limit)
                .with_memory_limit(current_config.memory_limit)
                .with_input(input_data)
                .build();

            let mut new_instance = vm
                .prepare(wasm_bytes, new_context, new_config)
                .expect("should prepare instance");

            // NOTE: We probably don't need to keep instances alive for repeated calls

            let function_index = {
                // let entry_points = manifest.entry_points;
                let entry_point = manifest
                    .entry_points
                    .into_iter()
                    .find_map(|entry_point| {
                        if entry_point.selector == selector {
                            Some(entry_point)
                        } else {
                            None
                        }
                    })
                    .expect("should find entry point");
                entry_point.function_index
            };

            // Call function by the index
            let (call_result, gas_usage) = new_instance.call_function(function_index);

            let (return_data, host_result) = match call_result {
                Ok(()) => {
                    // Contract did not call `casper_return` - implies that it did not revert, and
                    // did not pass any data.
                    (None, Ok(()))
                }
                Err(VMError::Return { flags, data }) => {
                    // If the contract called `casper_return` function with revert bit set, we have
                    // to pass the reverted code to the caller.
                    let host_result = if flags.contains(ReturnFlags::REVERT) {
                        Err(HostError::CalleeReverted)
                    } else {
                        Ok(())
                    };
                    (data, host_result)
                }
                Err(VMError::OutOfGas) => {
                    todo!()
                }
                Err(VMError::Trap(_trap)) => (None, Err(HostError::CalleeTrapped)),
                Err(other_error) => todo!("{other_error:?}"),
            };

            // Calculate how much gas was spent during execution of the called contract.
            let gas_spent = gas_usage
                .gas_limit
                .checked_sub(gas_usage.remaining_points)
                .expect("remaining points always below or equal to the limit");

            match caller.consume_gas(gas_spent) {
                MeteringPoints::Remaining(_) => {}
                MeteringPoints::Exhausted => {
                    todo!("exhausted")
                }
            }

            match &host_result {
                Ok(()) | Err(HostError::CalleeReverted) => {
                    // Called contract did return data by calling `casper_return` function.
                    if let Some(returned_data) = return_data {
                        let out_ptr: u32 = caller.alloc(cb_alloc, returned_data.len(), cb_ctx)?;
                        caller.memory_write(out_ptr, &returned_data)?;
                    }
                }
                Err(_other_host_error) => {}
            }

            match host_result {
                Ok(()) => {
                    // Execution succeeded, merge all effects into the caller.
                    let new_context = new_instance.teardown();
                    caller.context_mut().storage.merge(new_context.storage);
                }
                Err(HostError::CalleeGasDepleted)
                | Err(HostError::CalleeReverted)
                | Err(HostError::CalleeTrapped) => {
                    // Error means no changes to the storage
                }
            }

            Ok(host_result)
        }
        Some(stored_value) => {
            todo!("unsupported contract type: {stored_value:?}")
        }
        None => todo!("not found"),
    }
}

const CASPER_ENV_CALLER: u64 = 0;

pub(crate) fn casper_env_read<
    S: StateReader<Key, StoredValue, Error = global_state::error::Error>,
>(
    mut caller: impl Caller<S>,
    env_path: u32,
    env_path_length: u32,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> VMResult<u32> {
    // NOTE: Needs versioning to disallow situtation where a path contains values that may be used
    // in the future.

    let size: usize = env_path_length.try_into().unwrap();
    let env_path_bytes = caller.memory_read(env_path, size * 8)?;

    let path = match safe_transmute::transmute_many::<u64, SingleManyGuard>(&env_path_bytes) {
        Ok(env_path) => env_path,
        Err(safe_transmute::Error::Unaligned(unaligned_error))
            if unaligned_error.source.is_empty() =>
        {
            &[]
        }
        Err(error) => todo!("handle error {:?}", error),
    };

    let data = if path == &[CASPER_ENV_CALLER] {
        Some(caller.context().address.to_vec())
    } else {
        None
    };

    if let Some(data) = data {
        let out_ptr: u32 = if cb_alloc != 0 {
            caller.alloc(cb_alloc, data.len(), alloc_ctx)?
        } else {
            // treats alloc_ctx as data
            alloc_ctx
        };

        if out_ptr == 0 {
            Ok(out_ptr)
        } else {
            caller.memory_write(out_ptr, &data)?;
            Ok(out_ptr + (data.len() as u32))
        }
    } else {
        Ok(0)
    }
}

pub(crate) fn casper_env_caller<
    S: StateReader<Key, StoredValue, Error = global_state::error::Error>,
>(
    mut caller: impl Caller<S>,
    dest_ptr: u32,
    dest_len: u32,
) -> VMResult<u32> {
    let mut data = caller.context().address.as_slice();
    if dest_ptr == 0 {
        Ok(dest_ptr)
    } else {
        let dest_len = dest_len as usize;
        data = &data[0..cmp::min(32, dest_len as usize)];
        caller.memory_write(dest_ptr, &data)?;
        Ok(dest_ptr + (data.len() as u32))
    }
}
