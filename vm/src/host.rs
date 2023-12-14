//! Implementation of all host functions.
pub(crate) mod abi;

use std::{
    cell::RefCell,
    io::Cursor,
    mem,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use safe_transmute::SingleManyGuard;
use vm_common::flags::{EntryPointFlags, ReturnFlags};

use crate::{
    backend::{Caller, Context, MeteringPoints, WasmInstance},
    host::abi::{CreateResult, EntryPoint, Manifest, Param},
    storage::{self, Contract, Storage},
    ConfigBuilder, Error, VMError, VMResult, VM,
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
pub(crate) fn casper_write<S: Storage>(
    caller: impl Caller<S>,
    key_space: u64,
    key_ptr: u32,
    key_size: u32,
    value_tag: u64,
    value_ptr: u32,
    value_size: u32,
) -> i32 {
    let key = caller
        .memory_read(key_ptr, key_size.try_into().unwrap())
        .expect("should read key bytes");

    let value = caller
        .memory_read(value_ptr, value_size.try_into().unwrap())
        .expect("should read value bytes");

    // Write data to key value storage
    caller
        .context()
        .storage
        .write(key_space, &key, value_tag, &value)
        .unwrap();

    0
}

pub(crate) fn casper_print<S: Storage>(
    caller: impl Caller<S>,
    message_ptr: u32,
    message_size: u32,
) -> i32 {
    let vec = caller
        .memory_read(message_ptr, message_size.try_into().unwrap())
        .expect("should work");
    let msg = String::from_utf8_lossy(&vec);
    eprintln!("⛓️ {msg}");
    0
}

/// Write value under a key.
pub(crate) fn casper_read<S: Storage>(
    mut caller: impl Caller<S>,
    key_tag: u64,
    key_ptr: u32,
    key_size: u32,
    info_ptr: u32,
    cb_alloc: u32,
    cb_ctx: u32,
) -> Result<i32, VMError> {
    let key = caller
        .memory_read(key_ptr, key_size.try_into().unwrap())
        .expect("should read key bytes");

    match caller.context().storage.read(key_tag, &key) {
        Ok(Some(entry)) => {
            let out_ptr: u32 = caller
                .alloc(cb_alloc, entry.data.len(), cb_ctx)
                .unwrap_or_else(|error| panic!("{error:?}"));

            let read_info = ReadInfo {
                data: out_ptr,
                data_size: entry.data.len().try_into().unwrap(),
                tag: entry.tag,
            };

            let read_info_bytes = safe_transmute::transmute_one_to_bytes(&read_info);
            caller.memory_write(info_ptr, read_info_bytes).unwrap();
            caller.memory_write(out_ptr, &entry.data).unwrap();
            Ok(0)
        }
        Ok(None) => Ok(1),
        Err(_) => Ok(i32::MAX), // TODO: error handling
    }
}

pub(crate) fn casper_copy_input<S: Storage>(
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
        caller.memory_write(out_ptr, &input).unwrap();
        Ok(out_ptr + (input.len() as u32))
    }
}

/// Returns from the execution of a smart contract with an optional flags.
pub(crate) fn casper_return<S: Storage>(
    caller: impl Caller<S>,
    flags: u32,
    data_ptr: u32,
    data_len: u32,
) -> VMResult<()> {
    let flags = ReturnFlags::from_bits(flags).expect("should be valid flags"); // SAFETY: All unknown bits should be preserved.
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

pub(crate) fn casper_create_contract<S: Storage + 'static>(
    mut caller: impl Caller<S>,
    code_ptr: u32,
    code_len: u32,
    manifest_ptr: u32,
    entry_name: u32,
    entry_len: u32,
    input_ptr: u32,
    input_len: u32,
    result_ptr: u32,
) -> VMResult<HostResult> {
    let code = if code_ptr != 0 {
        caller
            .memory_read(code_ptr, code_len as usize)
            .map(Bytes::from)
            .unwrap()
    } else {
        caller.bytecode()
    };

    // For calling a constructor
    let entry_point_name = if entry_name == 0 {
        None
    } else {
        let data = caller.memory_read(entry_name, entry_len as _).unwrap();
        let str = String::from_utf8(data).expect("should be valid utf8");
        Some(str)
    };

    // Pass input data when calling a constructor. It's optional, as constructors aren't required
    let input_data: Option<Bytes> = if input_ptr == 0 {
        None
    } else {
        let input_data = caller.memory_read(input_ptr, input_len as _)?.into();
        Some(input_data)
    };

    let manifest = caller
        .memory_read(manifest_ptr, mem::size_of::<Manifest>())
        .unwrap();
    let bytes = manifest.as_slice();

    let manifest = match safe_transmute::transmute_one::<Manifest>(bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            todo!("handle error {:?}", error);
        }
    };

    let entry_points_bytes = caller
        .memory_read(
            manifest.entry_points_ptr,
            (manifest.entry_points_size as usize) * mem::size_of::<EntryPoint>(),
        )
        .unwrap();

    let entry_points =
        match safe_transmute::transmute_many::<EntryPoint, SingleManyGuard>(&entry_points_bytes) {
            Ok(entry_points) => entry_points,
            Err(error) => todo!("handle error {:?}", error),
        };

    dbg!(&entry_points);

    let entrypoints = {
        let mut vec = Vec::new();

        for entry_point in entry_points {
            let entry_point_name = caller
                .memory_read(entry_point.name_ptr, entry_point.name_len as usize)
                .unwrap();

            let params_bytes = caller
                .memory_read(
                    entry_point.params_ptr,
                    (entry_point.params_size as usize) * mem::size_of::<Param>(),
                )
                .unwrap();

            let mut params_vec = Vec::new();

            let params =
                match safe_transmute::transmute_many::<Param, SingleManyGuard>(&params_bytes) {
                    Ok(params) => params,
                    Err(safe_transmute::Error::Unaligned(unaligned_error))
                        if unaligned_error.source.is_empty() =>
                    {
                        &[]
                    }
                    Err(error) => {
                        todo!(
                            "unable to transmute {:?} params_bytes={:?}",
                            error,
                            params_bytes
                        )
                    }
                };

            for param in params {
                let name = caller
                    .memory_read(param.name_ptr, param.name_len as usize)
                    .unwrap();
                params_vec.push(storage::Param { name: name.into() });
            }

            vec.push(storage::EntryPoint {
                name: entry_point_name.into(),
                params: params_vec,
                function_index: entry_point.fptr,
                flags: EntryPointFlags::from_bits_truncate(entry_point.flags),
            })
        }

        vec
    };

    if let Some(entry_point_name) = entry_point_name {
        // Find all entrypoints with a flag set to CONSTRUCTOR
        let mut constructors = entrypoints
            .iter()
            .filter_map(|entrypoint| {
                if entrypoint.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                    Some(entrypoint)
                } else {
                    None
                }
            })
            .filter(|entry_point| entry_point.name == entry_point_name);

        // TODO: Should we validate amount of constructors or just rely on the fact that the proc
        // macro will statically check it, and the document the behavior that only first
        // constructor will be called

        match constructors.next() {
            Some(first_constructor) => {
                let mut vm = VM::new();
                let storage = caller.context().storage.clone();

                let current_config = caller.config().clone();
                let gas_limit = caller
                    .gas_consumed()
                    .try_into_remaining()
                    .expect("should be remaining");

                let new_context = Context { storage };

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

                match host_result {
                    Ok(()) => {
                        // We don't allow capturing output from a constructor. All constructors are
                        // expected to have return value of ().
                    }
                    error @ Err(_) => {
                        dbg!(&error);
                        // If the constructor failed, we have to return the error to the caller.
                        return Ok(error);
                    }
                }
            }
            None => {
                todo!("Constructor not found; raise error")
            }
        }
    }

    let manifest = storage::Manifest { entrypoints };

    dbg!(&manifest);

    let storage::CreateResult {
        package_address,
        contract_address,
    } = caller
        .context()
        .storage
        .create_contract(code, manifest)
        .unwrap();

    let create_result = CreateResult {
        package_address,
        contract_address,
        version: 1,
    };

    let create_result_bytes = safe_transmute::transmute_one_to_bytes(&create_result);

    debug_assert_eq!(
        safe_transmute::transmute_one(create_result_bytes),
        Ok(create_result),
        "Sanity check", // NOTE: Remove these guards with sufficient test coverage
    );

    caller
        .memory_write(result_ptr, create_result_bytes)
        .unwrap();

    Ok(Ok(()))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn casper_call<S: Storage + 'static>(
    mut caller: impl Caller<S>,
    address_ptr: u32,
    address_len: u32,
    value: u64,
    entry_name: u32,
    entry_len: u32,
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
    let address = caller.memory_read(address_ptr, address_len as _).unwrap();
    let entry_point_name = {
        let data = caller.memory_read(entry_name, entry_len as _).unwrap();
        String::from_utf8(data).expect("should be valid utf8")
    };
    let input_data = caller
        .memory_read(input_ptr, input_len as _)
        .unwrap()
        .into();

    let contract = caller.context().storage.read_contract(&address).unwrap();

    match contract {
        Some(Contract {
            code_hash,
            manifest,
        }) => {
            let wasm_bytes = caller.context().storage.read_code(&code_hash).unwrap();

            let mut vm = VM::new();
            let storage = caller.context().storage.clone();

            let new_context = Context { storage };

            // let config = ConfigBuilder:
            // caller.self.config()

            let current_config = caller.config().clone();

            // Take the gas spent so far and use it as a limit for the new VM.
            let gas_limit = caller
                .gas_consumed()
                .try_into_remaining()
                .expect("should be remaining");

            // let wasm_callbacks = Arc::new(WasmCallbacks::default());
            // let cloned = Arc::clone(&wasm_callbacks);

            let new_config = ConfigBuilder::new()
                .with_gas_limit(gas_limit)
                .with_memory_limit(current_config.memory_limit)
                .with_input(input_data)
                // .with_callback(cloned)
                .build();

            let mut new_instance = vm
                .prepare(wasm_bytes, new_context, new_config)
                .expect("should prepare instance");

            // NOTE: We probably don't need to keep instances alive for repeated calls

            let function_index = {
                let entry_points = manifest.entrypoints;
                let entry_point = entry_points
                    .iter()
                    .find_map(|entry_point| {
                        if entry_point.name == entry_point_name {
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
            dbg!(&return_data, &host_result);

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
            Ok(host_result)
        }
        None => todo!("not found"),
    }
}
