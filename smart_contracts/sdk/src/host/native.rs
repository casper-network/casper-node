use core::slice;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ptr::{self, NonNull},
    sync::{Arc, Mutex, RwLock},
};

use bytes::Bytes;
use casper_sdk_sys::for_each_host_function;
use once_cell::sync::Lazy;
use vm_common::flags::ReturnFlags;

use crate::types::Address;

macro_rules! define_trait_methods {
    ( @optional $ty:ty ) => { stringify!($ty) };
    ( @optional ) => { "()" };

    ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty,)* ))? $(-> $ret:ty)?;)+) => {
        $(
            $(#[$cfg])? fn $name(&self $($(,$arg: $argty)*)?) $(-> $ret)?;
        )*
    }
}

pub unsafe trait HostInterface {
    for_each_host_function!(define_trait_methods);
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct TaggedValue {
    tag: u64,
    value: Bytes,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct BorrowedTaggedValue<'a> {
    tag: u64,
    value: &'a [u8],
}
type Container = BTreeMap<u64, BTreeMap<Bytes, TaggedValue>>;

#[derive(Default, Clone, Debug)]
pub struct Stub {
    db: Arc<RwLock<Container>>,
    caller: Address,
}

impl Stub {
    pub fn new(db: Container, caller: Address) -> Self {
        Self {
            db: Arc::new(RwLock::new(db)),
            caller,
        }
    }
}

#[allow(unused_variables)]
unsafe impl HostInterface for Stub {
    fn casper_read(
        &self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        info: *mut casper_sdk_sys::ReadInfo,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> i32 {
        let key_bytes = unsafe { slice::from_raw_parts(key_ptr, key_size) };

        let mut db = self.db.read().unwrap();
        let value = match db.get(&key_space) {
            Some(values) => values.get(key_bytes).cloned(),
            None => return 1,
        };
        match value {
            Some(tagged_value) => {
                let ptr = NonNull::new(alloc(tagged_value.value.len(), alloc_ctx as _));

                if let Some(ptr) = ptr {
                    unsafe {
                        ptr::copy_nonoverlapping(
                            tagged_value.value.as_ptr(),
                            ptr.as_ptr(),
                            tagged_value.value.len(),
                        );
                    }
                }

                0
            }
            None => 1,
        }
    }

    fn casper_write(
        &self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        value_tag: u64,
        value_ptr: *const u8,
        value_size: usize,
    ) -> i32 {
        let key_bytes = unsafe { slice::from_raw_parts(key_ptr, key_size) };

        let value_bytes = unsafe { slice::from_raw_parts(value_ptr, value_size) };

        let mut db = self.db.write().unwrap();
        db.entry(key_space).or_default().insert(
            Bytes::copy_from_slice(key_bytes),
            TaggedValue {
                tag: value_tag,
                value: Bytes::copy_from_slice(value_bytes),
            },
        );
        0
    }

    fn casper_print(&self, msg_ptr: *const u8, msg_size: usize) -> i32 {
        let msg_bytes = unsafe { slice::from_raw_parts(msg_ptr, msg_size) };
        let msg = std::str::from_utf8(msg_bytes).expect("Valid UTF-8 string");
        println!("💻 {msg}");
        0
    }

    fn casper_return(&self, flags: u32, data_ptr: *const u8, data_len: usize) -> ! {
        let return_flags = ReturnFlags::from_bits(flags);
        let data = unsafe { slice::from_raw_parts(data_ptr, data_len) };
        panic!("revert with flags={return_flags:?} data={data:?}")
    }

    fn casper_copy_input(
        &self,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> *mut u8 {
        todo!()
    }

    fn casper_copy_output(&self, output_ptr: *const u8, output_len: usize) {
        todo!()
    }

    fn casper_create_contract(
        &self,
        code_ptr: *const u8,
        code_size: usize,
        manifest_ptr: *const casper_sdk_sys::Manifest,
        entry_point_ptr: *const u8,
        entry_point_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        result_ptr: *mut casper_sdk_sys::CreateResult,
    ) -> u32 {
        todo!()
    }

    fn casper_call(
        &self,
        address_ptr: *const u8,
        address_size: usize,
        value: u64,
        entry_point_ptr: *const u8,
        entry_point_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> u32 {
        todo!()
    }

    #[doc = r"Obtain data from the blockchain environemnt of current wasm invocation.

Example paths:

* `env_read([CASPER_CALLER], 1, nullptr, &caller_addr)` -> read caller's address into
  `caller_addr` memory.
* `env_read([CASPER_CHAIN, BLOCK_HASH, 0], 3, nullptr, &block_hash)` -> read hash of the
  current block into `block_hash` memory.
* `env_read([CASPER_CHAIN, BLOCK_HASH, 5], 3, nullptr, &block_hash)` -> read hash of the 5th
  block from the current one into `block_hash` memory.
* `env_read([CASPER_AUTHORIZED_KEYS], 1, nullptr, &authorized_keys)` -> read list of
  authorized keys into `authorized_keys` memory."]
    fn casper_env_read(
        &self,
        env_path: *const u64,
        env_path_size: usize,
        alloc: Option<extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8>,
        alloc_ctx: *const core::ffi::c_void,
    ) -> *mut u8 {
        todo!()
    }

    fn casper_env_caller(&self, dest: *mut u8, dest_size: usize) -> *const u8 {
        let dst = unsafe { slice::from_raw_parts_mut(dest, dest_size) };
        dst.copy_from_slice(&self.caller);
        unsafe { dest.add(32) }
    }
}

pub(crate) static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
pub(crate) static STUB: Lazy<RwLock<Stub>> = Lazy::new(|| RwLock::new(Stub::default()));

pub fn with_mock<Ret>(new_stub: Stub, func: impl FnOnce() -> Ret) -> Ret {
    let _lock = LOCK.lock().unwrap();
    {
        let mut stub = STUB.write().unwrap();
        *stub = new_stub;
    }
    func()
}

macro_rules! define_symbols {
    ( @optional $ty:ty ) => { stringify!($ty) };
    ( @optional ) => { "()" };

    ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty,)* ))? $(-> $ret:ty)?;)+) => {
        $(
            #[no_mangle]
            $(#[$cfg])? fn $name($($($arg: $argty,)*)?) $(-> $ret)? {
                let _name = stringify!($name);
                let _args = ($($(&$arg,)*)?);
                let _ret = define_symbols! { @optional $($ret)? };
                let mut stub = $crate::host::native::STUB.read().unwrap();
                stub.$name($($($arg,)*)?)
            }
        )*
    }
}

mod symbols {
    use super::HostInterface;
    use casper_sdk_sys::for_each_host_function;

    for_each_host_function!(define_symbols);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let msg = "Hello";
        let stub = STUB.read().unwrap();
        stub.casper_print(msg.as_ptr(), msg.len());
    }
}
