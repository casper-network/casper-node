//! Implementation of all host functions.
pub(crate) mod abi;

use std::{io::Cursor, mem};

use bytes::Bytes;
use safe_transmute::SingleManyGuard;

use crate::{
    backend::{Caller, Context, WasmInstance},
    host::abi::{CreateResult, EntryPoint, Manifest, Param},
    storage::{self, Contract, Storage},
    ConfigBuilder, HostError, VM,
};

use self::abi::ReadInfo;

#[derive(Debug)]
pub(crate) enum Outcome {
    /// Propagate VM error (i.e. after recursively calling into exports inside wasm).
    VM(Box<dyn std::error::Error>),
    /// A host error, i.e. a revert.
    Host(HostError),
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
) -> Result<i32, Outcome> {
    // caller.

    let key = caller
        .memory_read(key_ptr, key_size.try_into().unwrap())
        .expect("should read key bytes");

    match caller.context().storage.read(key_tag, &key) {
        Ok(Some(entry)) => {
            let out_ptr: u32 = caller
                .alloc(cb_alloc, entry.data.len(), cb_ctx)
                .map_err(Outcome::VM)?;

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

/// Write value under a key.
pub(crate) fn casper_revert(code: u32) -> i32 {
    todo!()
}

pub(crate) fn casper_copy_input<S: Storage>(
    mut caller: impl Caller<S>,
    cb_alloc: u32,
    alloc_ctx: u32,
) -> Result<u32, Outcome> {
    let input = caller.config().input.clone();

    let out_ptr: u32 = if cb_alloc != 0 {
        caller
            .alloc(cb_alloc, input.len(), alloc_ctx)
            .map_err(Outcome::VM)?
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

pub(crate) fn casper_create_contract<S: Storage>(
    mut caller: impl Caller<S>,
    code_ptr: u32,
    code_len: u32,
    manifest_ptr: u32,
    result_ptr: u32,
) -> Result<u32, Outcome> {
    let code = if code_ptr != 0 {
        caller
            .memory_read(code_ptr, code_len as usize)
            .map(Bytes::from)
            .unwrap()
    } else {
        caller.bytecode()
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
            dbg!(&params_bytes);
            for param in params {
                // dbg!(&param);
                let name = caller
                    .memory_read(param.name_ptr, param.name_len as usize)
                    .unwrap();
                dbg!(&name);

                params_vec.push(storage::Param {
                    name: name.into(),
                    ty: param.ty,
                });
            }

            vec.push(storage::EntryPoint {
                name: entry_point_name.into(),
                params: params_vec,
                function_index: entry_point.fptr,
            })
        }

        vec
    };

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

    Ok(0)
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
) -> Result<u32, Outcome> {
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

    if let Some(Contract {
        code_hash,
        manifest,
    }) = contract
    {
        let wasm_bytes = caller.context().storage.read_code(&code_hash).unwrap();

        let mut vm = VM::new();
        let storage = caller.context().storage.clone();

        let new_context = Context { storage };

        // let config = ConfigBuilder:
        // caller.self.config()

        let current_config = caller.config().clone();

        let new_config = ConfigBuilder::new()
            .with_gas_limit(current_config.gas_limit)
            .with_memory_limit(current_config.memory_limit)
            .with_input(input_data)
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

        let (call_result, gas_usage) = new_instance.call_function(function_index);
        dbg!(&call_result, &gas_usage);
    }

    Ok(0)
}
