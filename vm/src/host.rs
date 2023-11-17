//! Implementation of all host functions.
use std::{io::Cursor, mem};

use byteorder::{LittleEndian, ReadBytesExt};
use bytes::Bytes;

use crate::{
    backend::Caller,
    storage::{self, Entry, Storage},
    Error as VMError, HostError,
};

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

#[repr(C)]
struct ReadInfo {
    /// Allocated pointer.
    data: u32,
    /// Size in bytes.
    data_size: u32,
    /// Value tag.
    tag: u64,
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
                .map_err(|error| Outcome::VM(error))?;

            let read_info = ReadInfo {
                data: out_ptr,
                data_size: entry.data.len().try_into().unwrap(),
                tag: entry.tag,
            };

            let read_info_bytes: [u8; mem::size_of::<ReadInfo>()] =
                unsafe { mem::transmute_copy(&read_info) };
            dbg!(read_info_bytes.len());

            caller.memory_write(info_ptr, &read_info_bytes).unwrap();

            caller.memory_write(out_ptr, &entry.data).unwrap();

            // out_ptr
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
            .map_err(|error| Outcome::VM(error))?
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

#[repr(C)]
#[derive(Debug)]
pub struct EntryPoint {
    pub name_ptr: u32,
    pub name_len: u32,

    pub params_ptr: u32, // pointer of pointers (preferred 'static lifetime)
    pub params_size: u32,

    pub fptr: u32, // extern "C" fn(A1) -> (),
}

#[repr(C)]
#[derive(Debug)]
pub struct Manifest {
    entry_points_ptr: u32,
    entry_points_size: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct Param {
    pub name_ptr: u32,
    pub name_len: u32,
    pub ty: u32,
}

#[repr(C)]
#[derive(Debug)]

pub struct CreateResult {
    package_address: [u8; 32],
    contract_address: [u8; 32],
    version: u32,
}

pub(crate) fn casper_create_contract<S: Storage>(
    mut caller: impl Caller<S>,
    code_ptr: u32,
    code_len: u32,
    manifest_ptr: u32,
    result_ptr: u32,
) -> u32 {
    let code = if code_ptr != 0 {
        let code = caller
            .memory_read(code_ptr, code_len as usize)
            .map(Bytes::from)
            .unwrap();
        code
    } else {
        caller.bytecode()
    };

    let manifest = caller
        .memory_read(manifest_ptr, mem::size_of::<Manifest>())
        .unwrap();
    let bytes = manifest.as_slice();

    let manifest = {
        let mut rdr = Cursor::new(bytes);
        let entry_points_ptr = rdr.read_u32::<LittleEndian>().unwrap(); // TODO: Error handling
        let entry_points_size = rdr.read_u32::<LittleEndian>().unwrap(); // TODO: Error handling
        Manifest {
            entry_points_ptr,
            entry_points_size,
        }
    };

    let entry_points_bytes = caller
        .memory_read(
            manifest.entry_points_ptr,
            (manifest.entry_points_size as usize) * mem::size_of::<EntryPoint>(),
        )
        .unwrap();
    let (head, entry_points, tail) = unsafe { entry_points_bytes.align_to::<EntryPoint>() };

    let entrypoints = {
        let mut vec = Vec::new();

        for entry_point in entry_points {
            let entry_point_name = caller
                .memory_read(entry_point.name_ptr, entry_point.name_len as usize)
                .unwrap();
            // let entry_point_name = String::from_utf8(name).unwrap();
            // println!()

            let params_bytes = caller
                .memory_read(
                    entry_point.params_ptr,
                    (entry_point.params_size as usize) * mem::size_of::<Param>(),
                )
                .unwrap();

            let mut params_vec = Vec::new();
            let (head, params, tail) = unsafe { params_bytes.align_to::<Param>() };
            for param in params {
                let name = caller
                    .memory_read(param.name_ptr, param.name_len as usize)
                    .unwrap();

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

        // vec.push
        vec
    };

    let manifest = storage::Manifest { entrypoints };

    // caller.context().

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

    dbg!(&create_result);

    let create_result_bytes: [u8; mem::size_of::<CreateResult>()] =
        unsafe { mem::transmute_copy(&create_result) };

    caller
        .memory_write(result_ptr, &create_result_bytes)
        .unwrap();

    0
}

pub(crate) fn casper_call<S: Storage>(
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
    todo!()
}
