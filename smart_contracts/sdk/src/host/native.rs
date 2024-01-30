use bytes::Bytes;
use casper_sdk_sys::{for_each_host_function, Param};
use core::slice;
use once_cell::sync::Lazy;
use rand::Rng;
use std::{
    borrow::BorrowMut,
    cell::{Ref, RefCell},
    collections::{BTreeMap, VecDeque},
    convert::Infallible,
    ptr::{self, NonNull},
    sync::{Arc, Mutex, RwLock},
};
use vm_common::flags::{EntryPointFlags, ReturnFlags};

use crate::types::Address;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeTrap {
    Return(ReturnFlags, Bytes),
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
    // input_data: Arc<RwLock<Option<Bytes>>>,
    input_data: Option<Bytes>,
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
        &self,
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
        &self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        value_tag: u64,
        value_ptr: *const u8,
        value_size: usize,
    ) -> Result<i32, NativeTrap> {
        assert!(!key_ptr.is_null());
        assert!(!value_ptr.is_null());
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
    ) -> Result<Infallible, NativeTrap> {
        let return_flags = ReturnFlags::from_bits_truncate(flags);
        let data = unsafe { slice::from_raw_parts(data_ptr, data_len) };
        let data = Bytes::copy_from_slice(data);
        dbg!(&data);
        Err(NativeTrap::Return(return_flags, data))
    }

    fn casper_copy_input(
        &self,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> Result<*mut u8, NativeTrap> {
        let input_data = self.input_data.clone();
        let input_data = input_data.as_ref().cloned().unwrap_or_default();
        let ptr = NonNull::new(alloc(input_data.len(), alloc_ctx as _));

        match ptr {
            Some(ptr) => {
                if !input_data.is_empty() {
                    unsafe {
                        ptr::copy_nonoverlapping(
                            input_data.as_ptr(),
                            ptr.as_ptr(),
                            input_data.len(),
                        );
                    }
                }
                Ok(unsafe { ptr.as_ptr().add(input_data.len()) })
            }
            None => Ok(ptr::null_mut()),
        }
    }

    fn casper_copy_output(
        &self,
        output_ptr: *const u8,
        output_len: usize,
    ) -> Result<(), NativeTrap> {
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

            let mut stub = with_stub(|stub| stub);
            stub.caller = contract_address;
            stub.input_data = input_data.map(Bytes::copy_from_slice);

            // Call constructor, expect a trap
            let result = dispatch_with(stub, || {
                (entry_point.fptr)();
            });

            match result {
                Ok(()) => todo!("Constructor did not return"),
                Err(NativeTrap::Return(flags, bytes)) => {
                    if flags.contains(ReturnFlags::REVERT) {
                        todo!("Constructor returned with a revert flag");
                    }

                    let mut db = self.db.write().unwrap();
                    let values = db.entry(0).or_default();
                    values.insert(
                        Bytes::copy_from_slice(contract_address.as_slice()),
                        TaggedValue {
                            tag: 0,
                            value: bytes,
                        },
                    );
                }
            }
        }

        Ok(0)
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
    ) -> Result<u32, NativeTrap> {
        let address = unsafe { slice::from_raw_parts(address_ptr, address_size) };
        let entry_point_name = unsafe { slice::from_raw_parts(entry_point_ptr, entry_point_size) };
        let input_data = unsafe { slice::from_raw_parts(input_ptr, input_size) };
        dbg!(&input_data);
        let manifests = self.manifests.read().unwrap();
        let manifest = manifests.get(address).expect("Manifest exists");

        let entry_point = manifest
            .entry_points
            .iter()
            .find(|entry_point| entry_point.name.as_bytes() == entry_point_name)
            .expect("Entry point exists");

        // TODO: Wasm host should also forbid calling constructors
        assert!(
            !entry_point.flags.contains(EntryPointFlags::CONSTRUCTOR),
            "Calling constructors is unsupported"
        );

        // self.input_data

        let mut new_stub = with_stub(|stub| stub.clone());
        new_stub.input_data = Some(Bytes::copy_from_slice(input_data));

        match dispatch_with(new_stub, || {
            (entry_point.fptr)();
        }) {
            Ok(()) => Ok(0),
            Err(NativeTrap::Return(flags, bytes)) => {
                if !flags.contains(ReturnFlags::REVERT) {
                    // TODO: This is currently really bad simplification and only means that when a
                    // casper_return is called it will not update the state, but all other
                    // casper_write calls succeeded and are observable.

                    let mut db = self.db.write().unwrap();
                    let values = db.entry(0).or_default();
                    values.insert(
                        Bytes::copy_from_slice(address),
                        TaggedValue {
                            tag: 0,
                            value: bytes.clone(),
                        },
                    );
                }

                let ptr = NonNull::new(alloc(bytes.len(), alloc_ctx as _));
                if let Some(output_ptr) = ptr {
                    unsafe {
                        ptr::copy_nonoverlapping(bytes.as_ptr(), output_ptr.as_ptr(), bytes.len());
                    }
                }

                Ok(0)
            }
        }
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
    ) -> Result<*mut u8, NativeTrap> {
        todo!()
    }

    fn casper_env_caller(&self, dest: *mut u8, dest_size: usize) -> Result<*const u8, NativeTrap> {
        let dst = unsafe { slice::from_raw_parts_mut(dest, dest_size) };
        dst.copy_from_slice(&self.caller);
        Ok(unsafe { dest.add(32) })
    }
}

thread_local! {
    pub(crate) static LAST_TRAP: RefCell<Option<NativeTrap>> = RefCell::new(None);
    static STUB_STACK: RefCell<VecDeque<Stub>> = RefCell::new(VecDeque::new());
}

pub fn with_stub<T>(f: impl FnOnce(Stub) -> T) -> T {
    STUB_STACK.with(|stack| {
        let stub = {
            let borrowed = stack.borrow();
            let front = borrowed.front().expect("Stub exists").clone();
            front
        };
        f(stub)
    })
}

fn handle_ret_with<T>(value: Result<T, NativeTrap>, ret: impl FnOnce() -> T) -> T {
    match value {
        Ok(result) => {
            LAST_TRAP.with(|last_trap| last_trap.borrow_mut().take());
            result
        }
        Err(trap) => {
            let result = ret();
            LAST_TRAP.with(|last_trap| last_trap.borrow_mut().replace(trap));
            result
        }
    }
}

fn handle_ret<T: Default>(value: Result<T, NativeTrap>) -> T {
    handle_ret_with(value, || Default::default())
}

pub fn dispatch_with<T>(stub: Stub, f: impl FnOnce() -> T) -> Result<T, NativeTrap> {
    STUB_STACK.with(|stack| {
        let mut borrowed = stack.borrow_mut();
        borrowed.push_front(stub);
    });

    // Clear previous trap (if present)
    LAST_TRAP.with(|last_trap| last_trap.borrow_mut().take());

    // Call a function
    let result = f();

    // Check if a trap was set and return it if so (otherwise return the result).
    let last_trap = LAST_TRAP.with(|last_trap| last_trap.borrow_mut().take());

    let result = if let Some(trap) = last_trap {
        Err(trap)
    } else {
        Ok(result)
    };

    // Pop the stub from the stack
    STUB_STACK.with(|stack| {
        let mut borrowed = stack.borrow_mut();
        borrowed.pop_front();
    });

    result
}

macro_rules! define_symbols {

    ( @optional $ty:ty ) => { $ty };
    ( @optional ) => { () };

    ( $( $(#[$cfg:meta])? $vis:vis fn $name:ident $(( $($arg:ident: $argty:ty,)* ))? $(-> $ret:ty)?;)+) => {
        $(
            mod $name {
                type Ret = define_symbols!(@optional $($ret)?);

                #[no_mangle]
                $(#[$cfg])? pub extern "C"  fn $name($($($arg: $argty,)*)?) -> Ret {
                    let _name = stringify!($name);
                    let _args = ($($(&$arg,)*)?);

                    let _call_result = $crate::host::native::STUB.with(|stub| {
                        let stub = stub.borrow();
                        stub.$name($($($arg,)*)?)
                    });

                    $crate::host::native::handle_ret(_call_result)
                }
            }

        )*
    }
}

mod symbols {
    // TODO: Figure out how to use for_each_host_function macro here and deal with never type in
    // casper_return
    #[no_mangle]
    ///Read value from a storage available for caller's entity address.
    pub extern "C" fn casper_read(
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        info: *mut ::casper_sdk_sys::ReadInfo,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> i32 {
        let _name = "casper_read";
        let _args = (&key_space, &key_ptr, &key_size, &info, &alloc, &alloc_ctx);
        let _call_result = with_stub(|stub| {
            stub.casper_read(key_space, key_ptr, key_size, info, alloc, alloc_ctx)
        });
        crate::host::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_write(
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        value_tag: u64,
        value_ptr: *const u8,
        value_size: usize,
    ) -> i32 {
        let _name = "casper_write";
        let _args = (
            &key_space,
            &key_ptr,
            &key_size,
            &value_tag,
            &value_ptr,
            &value_size,
        );
        let _call_result = with_stub(|stub| {
            stub.casper_write(
                key_space, key_ptr, key_size, value_tag, value_ptr, value_size,
            )
        });
        crate::host::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_print(msg_ptr: *const u8, msg_size: usize) -> i32 {
        let _name = "casper_print";
        let _args = (&msg_ptr, &msg_size);
        let _call_result = with_stub(|stub| stub.casper_print(msg_ptr, msg_size));
        crate::host::native::handle_ret(_call_result)
    }

    use crate::host::native::LAST_TRAP;

    #[no_mangle]
    pub extern "C" fn casper_return(flags: u32, data_ptr: *const u8, data_len: usize) {
        let _name = "casper_return";
        let _args = (&flags, &data_ptr, &data_len);
        let _call_result = with_stub(|stub| stub.casper_return(flags, data_ptr, data_len));
        let err = _call_result.unwrap_err(); // SAFE
        dbg!(&err);
        LAST_TRAP.with(|last_trap| last_trap.borrow_mut().replace(err));
    }

    #[no_mangle]
    pub extern "C" fn casper_copy_input(
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> *mut u8 {
        let _name = "casper_copy_input";
        let _args = (&alloc, &alloc_ctx);
        let _call_result = with_stub(|stub| stub.casper_copy_input(alloc, alloc_ctx));
        crate::host::native::handle_ret_with(_call_result, || ptr::null_mut())
    }

    #[no_mangle]
    pub extern "C" fn casper_copy_output(output_ptr: *const u8, output_len: usize) {
        let _name = "casper_copy_output";
        let _args = (&output_ptr, &output_len);
        let _call_result = with_stub(|stub| stub.casper_copy_output(output_ptr, output_len));
        crate::host::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_create_contract(
        code_ptr: *const u8,
        code_size: usize,
        manifest_ptr: *const ::casper_sdk_sys::Manifest,
        entry_point_ptr: *const u8,
        entry_point_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        result_ptr: *mut ::casper_sdk_sys::CreateResult,
    ) -> u32 {
        let _name = "casper_create_contract";
        let _args = (
            &code_ptr,
            &code_size,
            &manifest_ptr,
            &entry_point_ptr,
            &entry_point_size,
            &input_ptr,
            &input_size,
            &result_ptr,
        );
        let _call_result = with_stub(|stub| {
            stub.casper_create_contract(
                code_ptr,
                code_size,
                manifest_ptr,
                entry_point_ptr,
                entry_point_size,
                input_ptr,
                input_size,
                result_ptr,
            )
        });
        crate::host::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_call(
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
        let _name = "casper_call";
        let _args = (
            &address_ptr,
            &address_size,
            &value,
            &entry_point_ptr,
            &entry_point_size,
            &input_ptr,
            &input_size,
            &alloc,
            &alloc_ctx,
        );
        let _call_result = with_stub(|stub| {
            stub.casper_call(
                address_ptr,
                address_size,
                value,
                entry_point_ptr,
                entry_point_size,
                input_ptr,
                input_size,
                alloc,
                alloc_ctx,
            )
        });
        crate::host::native::handle_ret(_call_result)
    }

    use std::ptr;

    use super::with_stub;

    #[no_mangle]
    /**Obtain data from the blockchain environemnt of current wasm invocation.

    Example paths:

    * `env_read([CASPER_CALLER], 1, nullptr, &caller_addr)` -> read caller's address into
    `caller_addr` memory.
    * `env_read([CASPER_CHAIN, BLOCK_HASH, 0], 3, nullptr, &block_hash)` -> read hash of the
    current block into `block_hash` memory.
    * `env_read([CASPER_CHAIN, BLOCK_HASH, 5], 3, nullptr, &block_hash)` -> read hash of the 5th
    block from the current one into `block_hash` memory.
    * `env_read([CASPER_AUTHORIZED_KEYS], 1, nullptr, &authorized_keys)` -> read list of
    authorized keys into `authorized_keys` memory.*/
    pub extern "C" fn casper_env_read(
        env_path: *const u64,
        env_path_size: usize,
        alloc: Option<extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8>,
        alloc_ctx: *const core::ffi::c_void,
    ) -> *mut u8 {
        let _name = "casper_env_read";
        let _args = (&env_path, &env_path_size, &alloc, &alloc_ctx);
        let _call_result =
            with_stub(|stub| stub.casper_env_read(env_path, env_path_size, alloc, alloc_ctx));
        crate::host::native::handle_ret_with(_call_result, || ptr::null_mut())
    }

    #[no_mangle]
    pub extern "C" fn casper_env_caller(dest: *mut u8, dest_len: usize) -> *const u8 {
        let _name = "casper_env_caller";
        let _args = (&dest, &dest_len);
        let _call_result = with_stub(|stub| stub.casper_env_caller(dest, dest_len));
        crate::host::native::handle_ret_with(_call_result, || ptr::null())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let msg = "Hello";
        // let stub = STUB.read().unwrap();
        // stub.casper_print(msg.as_ptr(), msg.len());
        let res = with_stub(|stub| stub.casper_print(msg.as_ptr(), msg.len())).expect("Ok");
        assert_eq!(res, 0);
    }
}
