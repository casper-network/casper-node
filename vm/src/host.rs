//! Implementation of all host functions.
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
