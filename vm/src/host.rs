//! Implementation of all host functions.
use std::mem;

use bytes::{BufMut, BytesMut};
use wasmer::WasmPtr;

use crate::{backend::Caller, storage::Storage};

/// Write value under a key.
pub(crate) fn casper_write<S: Storage>(
    caller: impl Caller<S>,
    _key_space: u64,
    key_ptr: u32,
    key_size: u32,
    _value_tag: u64,
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
    caller.context().storage.write(&key, &value).unwrap();

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
    eprintln!("💬 {msg}");
    0
}

#[repr(C)]
struct ReadInfo {
    /// Size in bytes.
    size: u64,
    /// Value tag.
    tag: u64,
}

/// Write value under a key.
pub(crate) fn casper_read<S: Storage>(
    mut caller: impl Caller<S>,
    _key_space: u64,
    key_ptr: u32,
    key_size: u32,
    info_ptr: u32,
) -> u32 {
    let key = caller
        .memory_read(key_ptr, key_size.try_into().unwrap())
        .expect("should read key bytes");

    match caller.context().storage.read(&key) {
        Ok(Some(value)) => {
            // let info = ReadInfo {
            //     size: value.len(),
            //     tag: u64::MAX, // TODO: placeholder
            // };
            // let info_bytes = [0u8; mem::size_of::<ReadInfo>()];

            // let mut info = BytesMut::with_capacity(mem::size_of::<ReadInfo>()); // TODO: fix, use
            // C ABI, consider using wasmer::WasmPtr info.put_u64(value.len().
            // try_into().unwrap()); info.put_u64(u64::MAX); // TODO: placeholder
            let read_info = ReadInfo {
                size: value.len().try_into().unwrap(),
                tag: u64::MAX, // TODO: placeholder
            };
            let read_info_bytes: [u8; mem::size_of::<ReadInfo>()] =
                unsafe { mem::transmute_copy(&read_info) };
            caller.memory_write(info_ptr, &read_info_bytes).unwrap();

            let out_ptr: u32 = caller.alloc(value.len());
            caller.memory_write(out_ptr, &value).unwrap();

            out_ptr
        }
        Ok(None) => 0,
        Err(_) => u32::MAX, // TODO: error handling
    }
}
