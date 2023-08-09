//! Implementation of all host functions.
use std::mem;

use bytes::{BufMut, BytesMut};
use wasmer::WasmPtr;

use crate::{backend::Caller, storage::Storage, Error as VMError, HostError};

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
) -> Result<i32, Outcome> {
    let key = caller
        .memory_read(key_ptr, key_size.try_into().unwrap())
        .expect("should read key bytes");

    match caller.context().storage.read(key_tag, &key) {
        Ok(Some(entry)) => {
            let out_ptr: u32 = caller
                .alloc(entry.data.len())
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
