use bytes::Bytes;
use casper_sdk_sys::{for_each_host_function, Param};
use core::slice;
use once_cell::sync::Lazy;
use rand::Rng;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ptr::{self, NonNull},
    sync::{Arc, Mutex, RwLock},
};
use vm_common::flags::{EntryPointFlags, ReturnFlags};

use crate::types::Address;

#[derive(Default)]
pub struct Never(());

pub enum NativeTrap {
    Revert(ReturnFlags, Bytes),
}

macro_rules! define_trait_methods {
    // ( @optional $ty:ty ) => { stringify!($ty) };
    // ( @optional ) => { "()" };
    ( @ret $ty:ty ) => { Result<$ty, $crate::host::native::NativeTrap> };
    ( @ret !) => { Result<$crate::host::native::Never, $crate::host::native::NativeTrap> };
    ( @ret ) => { Result<(), $crate::host::native::NativeTrap> };

    ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty,)* ))? $(-> $ret:ty)?;)+) => {
        $(
            $(#[$cfg])? fn $name(&self $($(,$arg: $argty)*)?) -> define_trait_methods!(@ret $($ret)?);
        )*
    }
}

// pub unsafe trait HostInterface {
//     for_each_host_function!(define_trait_methods);
// }
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

#[derive(Clone, Debug)]
pub struct NativeParam(String);

impl Into<NativeParam> for &casper_sdk_sys::Param {
    fn into(self) -> NativeParam {
        let name =
            String::from_utf8_lossy(unsafe { slice::from_raw_parts(self.name_ptr, self.name_len) })
                .into_owned();
        NativeParam(name)
    }
}

#[derive(Clone)]
pub struct NativeEntryPoint {
    pub name: String,
    pub params: Vec<NativeParam>,
    pub fptr: Arc<dyn Fn() -> () + Send + Sync>,
    pub flags: EntryPointFlags,
}

impl std::fmt::Debug for NativeEntryPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeEntryPoint")
            .field("name", &self.name)
            .field("params", &self.params)
            .field("fptr", &"<fptr>")
            .field("flags", &self.flags)
            .finish()
    }
}

impl Into<NativeEntryPoint> for &casper_sdk_sys::EntryPoint {
    fn into(self) -> NativeEntryPoint {
        let name =
            String::from_utf8_lossy(unsafe { slice::from_raw_parts(self.name_ptr, self.name_len) })
                .into_owned();
        let params = unsafe { slice::from_raw_parts(self.params_ptr, self.params_size) }
            .iter()
            .map(|param| param.into())
            .collect();
        let ptr = self.fptr;
        let fptr = Arc::new(move || {
            ptr();
        });
        let flags = EntryPointFlags::from_bits(self.flags).expect("Valid flags");
        NativeEntryPoint {
            name,
            params,
            fptr,
            flags,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct NativeManifest {
    pub entry_points: Vec<NativeEntryPoint>,
}

impl Into<NativeManifest> for NonNull<casper_sdk_sys::Manifest> {
    fn into(self) -> NativeManifest {
        let manifest = unsafe { self.as_ref() };
        let entry_points =
            unsafe { slice::from_raw_parts(manifest.entry_points, manifest.entry_points_size) }
                .iter()
                .map(|entry_point| entry_point.into())
                .collect();
        NativeManifest { entry_points }
    }
}

#[derive(Default, Clone, Debug)]
pub struct Stub {
    db: Arc<RwLock<Container>>,
    manifests: Arc<RwLock<BTreeMap<Address, NativeManifest>>>,
    input_data: Arc<RwLock<Option<Bytes>>>,
    caller: Address,
}

impl Stub {
    pub fn new(db: Container, caller: Address) -> Self {
        Self {
            db: Arc::new(RwLock::new(db)),
            manifests: Arc::new(RwLock::new(BTreeMap::new())),
            input_data: Default::default(),
            caller,
        }
    }
}

impl Stub {
    fn casper_read(
        &mut self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        info: *mut casper_sdk_sys::ReadInfo,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> Result<i32, NativeTrap> {
        let key_bytes = unsafe { slice::from_raw_parts(key_ptr, key_size) };

        let mut db = self.db.read().unwrap();
        let value = match db.get(&key_space) {
            Some(values) => values.get(key_bytes).cloned(),
            None => return Ok(1),
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

                Ok(0)
            }
            None => Ok(1),
        }
    }

    fn casper_write(
        &mut self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        value_tag: u64,
        value_ptr: *const u8,
        value_size: usize,
    ) -> Result<i32, NativeTrap> {
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
        Ok(0)
    }

    fn casper_print(&self, msg_ptr: *const u8, msg_size: usize) -> Result<i32, NativeTrap> {
        let msg_bytes = unsafe { slice::from_raw_parts(msg_ptr, msg_size) };
        let msg = std::str::from_utf8(msg_bytes).expect("Valid UTF-8 string");
        println!("💻 {msg}");
        Ok(0)
    }

    fn casper_return(
        &self,
        flags: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> Result<Never, NativeTrap> {
        let return_flags = ReturnFlags::from_bits_truncate(flags);
        let data = unsafe { slice::from_raw_parts(data_ptr, data_len) };
        Err(NativeTrap::Revert(
            return_flags,
            Bytes::copy_from_slice(data),
        ))
    }

    fn casper_copy_input(
        &mut self,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> Result<*mut u8, NativeTrap> {
        let input_data = self.input_data.read().unwrap();
        let input_data = input_data.as_ref().cloned().unwrap_or_default();
        let ptr = NonNull::new(alloc(input_data.len(), alloc_ctx as _));

        if let Some(ptr) = ptr {
            unsafe {
                ptr::copy_nonoverlapping(input_data.as_ptr(), ptr.as_ptr(), input_data.len());
            }
        }

        Ok(ptr.map(|ptr| ptr.as_ptr()).unwrap_or(ptr::null_mut()))
    }

    fn casper_copy_output(
        &self,
        output_ptr: *const u8,
        output_len: usize,
    ) -> Result<(), NativeTrap> {
        todo!()
    }

    fn casper_create_contract(
        &mut self,
        code_ptr: *const u8,
        code_size: usize,
        manifest_ptr: *const casper_sdk_sys::Manifest,
        entry_point_ptr: *const u8,
        entry_point_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        result_ptr: *mut casper_sdk_sys::CreateResult,
    ) -> Result<u32, NativeTrap> {
        let manifest =
            NonNull::new(manifest_ptr as *mut casper_sdk_sys::Manifest).expect("Manifest instance");
        let code = if code_ptr.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(code_ptr, code_size) })
        };

        if code.is_some() {
            panic!("Supplying code is not supported yet in native mode");
        }

        let entry_point: Option<&str> = if entry_point_ptr.is_null() {
            None
        } else {
            Some(unsafe {
                std::str::from_utf8_unchecked(slice::from_raw_parts(
                    entry_point_ptr,
                    entry_point_size,
                ))
            })
        };

        let input_data = if input_ptr.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(input_ptr, input_size) })
        };

        let mut rng = rand::thread_rng();
        let contract_address = rng.gen();
        let package_address = rng.gen();

        let mut result = NonNull::new(result_ptr).expect("Valid pointer");
        unsafe {
            result.as_mut().contract_address = contract_address;
            result.as_mut().package_address = package_address;
        }

        let mut manifests = self.manifests.write().unwrap();
        manifests.insert(contract_address, manifest.into());

        if let Some(entry_point_name) = entry_point {
            let manifest = manifests.get(&contract_address).expect("Manifest exists");
            let entry_point = manifest
                .entry_points
                .iter()
                .find(|entry_point| entry_point.name == entry_point_name)
                .expect("Entry point exists");
            if let Some(input_data) = input_data {
                let mut data = self.input_data.write().unwrap();
                data.replace(Bytes::copy_from_slice(input_data));
            }
            (entry_point.fptr)();
        }

        Ok(0)
    }

    fn casper_call(
        &mut self,
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
        &mut self,
        env_path: *const u64,
        env_path_size: usize,
        alloc: Option<extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8>,
        alloc_ctx: *const core::ffi::c_void,
    ) -> Result<*mut u8, NativeTrap> {
        todo!()
    }

    fn casper_env_caller(&self, dest: *mut u8, dest_size: usize) -> *const u8 {
        let dst = unsafe { slice::from_raw_parts_mut(dest, dest_size) };
        dst.copy_from_slice(&self.caller);
        unsafe { dest.add(32) }
    }
}

/// Stores last result after invoking a symbol.
///
/// NOTE: A bit hacky, but it will works for now.
pub(crate) static LAST_TRAP: Lazy<RwLock<Option<NativeTrap>>> = Lazy::new(|| RwLock::new(None));
pub(crate) static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
// pub(crate) static STUB: Lazy<RwLock<Stub>> = Lazy::new(|| RwLock::new(Stub::default()));

thread_local! {
    pub static STUB: RefCell<Stub> = RefCell::new(Stub::default());
}

macro_rules! define_symbols {
    ( @optional $ty:ty ) => { Result<$ty, $crate::host::native::NativeTrap> };
    ( @optional ) => { Result<(), $crate::host::native::NativeTrap> };

    ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty,)* ))? $(-> $ret:ty)?;)+) => {
        $(
            $(#[$cfg])? fn $name($($($arg: $argty,)*)?) -> define_symbols!(@optional $($ret)?) {
                let _name = stringify!($name);
                let _args = ($($(&$arg,)*)?);

                // define_symbols! { @optional $($ret)? }
                let call_result = $crate::host::native::STUB.with(|stub| {
                    let mut stub = stub.borrow_mut();
                    stub.$name($($($arg,)*)?)
                });

            }
        )*
    }
}

mod symbols {
    // use super::HostInterface;
    use casper_sdk_sys::for_each_host_function;
    // for_each_host_function!(define_symbols);
    // for_each_host_function!(define_symbols);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let msg = "Hello";
        // let stub = STUB.read().unwrap();
        // stub.casper_print(msg.as_ptr(), msg.len());
    }
}
