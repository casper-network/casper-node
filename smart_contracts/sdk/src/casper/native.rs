use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    convert::Infallible,
    fmt,
    os::raw::c_void,
    panic::{self, UnwindSafe},
    ptr::{self, NonNull},
    slice,
    sync::{Arc, RwLock},
};

use bytes::Bytes;
use casper_executor_wasm_common::{
    error::{
        CALLEE_REVERTED, CALLEE_SUCCEEDED, CALLEE_TRAPPED, HOST_ERROR_INTERNAL,
        HOST_ERROR_NOT_FOUND, HOST_ERROR_SUCCESS,
    },
    flags::ReturnFlags,
};
use once_cell::sync::Lazy;
use rand::Rng;

use super::Entity;
use crate::types::Address;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExportKind {
    SmartContract {
        struct_name: &'static str,
        name: &'static str,
    },
    TraitImpl {
        trait_name: &'static str,
        impl_name: &'static str,
        name: &'static str,
    },
    Function {
        name: &'static str,
    },
}

impl ExportKind {
    pub fn name(&self) -> &'static str {
        match self {
            ExportKind::SmartContract { name, .. }
            | ExportKind::TraitImpl { name, .. }
            | ExportKind::Function { name } => name,
        }
    }
}

pub struct Export {
    pub kind: ExportKind,
    pub fptr: fn() -> (),
    pub module_path: &'static str,
    pub file: &'static str,
    pub line: u32,
}

#[doc(hidden)]
pub mod private_exports {
    use super::Export;
    use linkme::distributed_slice;

    #[distributed_slice]
    #[linkme(crate = crate::linkme)]
    pub static EXPORTS: [Export];
}

/// List of sorted exports gathered from the contracts code.
pub static EXPORTS: Lazy<Vec<&'static Export>> = Lazy::new(|| {
    let mut exports = private_exports::EXPORTS.into_iter().collect::<Vec<_>>();
    exports.sort_by_key(|export| export.kind);
    exports
});

impl fmt::Debug for Export {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            kind,
            fptr: _,
            module_path,
            file,
            line,
        } = self;

        f.debug_struct("Export")
            .field("kind", kind)
            .field("fptr", &"<fptr>")
            .field("module_path", module_path)
            .field("file", file)
            .field("line", line)
            .finish()
    }
}

pub fn call_export(name: &str) {
    let exports_by_name: Vec<_> = EXPORTS
        .iter()
        .filter(|export|
            matches!(export.kind, ExportKind::Function { name: export_name } if export_name == name)
        )
        .collect();

    assert_eq!(exports_by_name.len(), 1);

    (exports_by_name[0].fptr)();
}

#[derive(Debug)]
pub enum NativeTrap {
    Return(ReturnFlags, Bytes),
    Panic(Box<dyn std::any::Any + Send + 'static>),
}

pub type Container = BTreeMap<u64, BTreeMap<Bytes, Bytes>>;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct NativeParam(pub(crate) String);

impl From<&casper_contract_sdk_sys::Param> for NativeParam {
    fn from(val: &casper_contract_sdk_sys::Param) -> Self {
        let name =
            String::from_utf8_lossy(unsafe { slice::from_raw_parts(val.name_ptr, val.name_len) })
                .into_owned();
        NativeParam(name)
    }
}

#[derive(Clone, Debug)]
pub struct Environment {
    pub db: Arc<RwLock<Container>>,
    contracts: Arc<RwLock<BTreeSet<Address>>>,
    // input_data: Arc<RwLock<Option<Bytes>>>,
    input_data: Option<Bytes>,
    contract_address: Option<Address>,
    caller: Entity,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            db: Default::default(),
            contracts: Default::default(),
            input_data: Default::default(),
            contract_address: Default::default(),
            caller: DEFAULT_ADDRESS,
        }
    }
}

pub const DEFAULT_ADDRESS: Entity = Entity::Account([42; 32]);

impl Environment {
    #[must_use]
    pub fn new(db: Container, caller: Entity) -> Self {
        Self {
            db: Arc::new(RwLock::new(db)),
            contracts: Default::default(),
            input_data: Default::default(),
            contract_address: None,
            caller,
        }
    }

    #[must_use]
    pub fn with_caller(&self, caller: Entity) -> Self {
        let mut env = self.clone();
        env.caller = caller;
        env
    }

    #[must_use]
    pub fn with_input_data(&self, input_data: Vec<u8>) -> Self {
        let mut env = self.clone();
        env.input_data = Some(Bytes::from(input_data));
        env
    }

    fn casper_env_transferred_value(&self, dest: *mut core::ffi::c_void) -> Result<(), NativeTrap> {
        let dest_ptr = NonNull::new(dest).expect("Valid pointer");
        let value = 0u128;
        let src_ptr = NonNull::from(&value);
        unsafe {
            ptr::copy_nonoverlapping(src_ptr.as_ptr() as *const c_void, dest_ptr.as_ptr(), 16)
        }
        Ok(())
    }
}

impl Environment {
    fn key_prefix(&self, key: &[u8]) -> Vec<u8> {
        let entity = self
            .contract_address
            .map(Entity::Contract)
            .unwrap_or(self.caller);

        let mut bytes = Vec::new();
        bytes.extend(entity.tag().to_le_bytes());
        bytes.extend(entity.address());
        bytes.extend(key);

        bytes
    }

    fn casper_read(
        &self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        info: *mut casper_contract_sdk_sys::ReadInfo,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> Result<u32, NativeTrap> {
        let key_bytes = unsafe { slice::from_raw_parts(key_ptr, key_size) };
        let key_bytes = self.key_prefix(key_bytes);

        let Ok(db) = self.db.read() else {
            return Ok(HOST_ERROR_INTERNAL);
        };

        let value = match db.get(&key_space) {
            Some(values) => values.get(key_bytes.as_slice()).cloned(),
            None => return Ok(HOST_ERROR_NOT_FOUND),
        };
        match value {
            Some(tagged_value) => {
                let ptr = NonNull::new(alloc(tagged_value.len(), alloc_ctx as _));

                if let Some(ptr) = ptr {
                    unsafe {
                        (*info).data = ptr.as_ptr();
                        (*info).size = tagged_value.len();
                    }

                    unsafe {
                        ptr::copy_nonoverlapping(
                            tagged_value.as_ptr(),
                            ptr.as_ptr(),
                            tagged_value.len(),
                        );
                    }
                }

                Ok(HOST_ERROR_SUCCESS)
            }
            None => Ok(HOST_ERROR_NOT_FOUND),
        }
    }

    fn casper_write(
        &self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        value_ptr: *const u8,
        value_size: usize,
    ) -> Result<u32, NativeTrap> {
        assert!(!key_ptr.is_null());
        assert!(!value_ptr.is_null());
        // let key_bytes = unsafe { slice::from_raw_parts(key_ptr, key_size) };
        let key_bytes = unsafe { slice::from_raw_parts(key_ptr, key_size) }.to_owned();
        let key_bytes = self.key_prefix(&key_bytes);

        let value_bytes = unsafe { slice::from_raw_parts(value_ptr, value_size) };

        let mut db = self.db.write().unwrap();
        db.entry(key_space).or_default().insert(
            Bytes::from(key_bytes.to_vec()),
            Bytes::from(value_bytes.to_vec()),
        );
        Ok(HOST_ERROR_SUCCESS)
    }

    fn casper_print(&self, msg_ptr: *const u8, msg_size: usize) -> Result<(), NativeTrap> {
        let msg_bytes = unsafe { slice::from_raw_parts(msg_ptr, msg_size) };
        let msg = std::str::from_utf8(msg_bytes).expect("Valid UTF-8 string");
        println!("💻 {msg}");
        Ok(())
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

    #[allow(clippy::too_many_arguments)]
    fn casper_create(
        &self,
        code_ptr: *const u8,
        code_size: usize,
        transferred_value_ptr: *const c_void,
        constructor_ptr: *const u8,
        constructor_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        result_ptr: *mut casper_contract_sdk_sys::CreateResult,
    ) -> Result<u32, NativeTrap> {
        // let manifest =
        //     NonNull::new(manifest_ptr as *mut casper_contract_sdk_sys::Manifest).expect("Manifest
        // instance");
        let code = if code_ptr.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(code_ptr, code_size) })
        };

        if code.is_some() {
            panic!("Supplying code is not supported yet in native mode");
        }

        let constructor = if constructor_ptr.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(constructor_ptr, constructor_size) })
        };

        let input_data = if input_ptr.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(input_ptr, input_size) })
        };

        let transferred_value: u128 = {
            let value_ptr =
                NonNull::new(transferred_value_ptr as *mut u128).expect("Valid pointer");
            unsafe { *value_ptr.as_ptr() }
        };

        assert_eq!(
            transferred_value, 0,
            "Creating new contracts with transferred value is not supported in native mode"
        );

        let mut rng = rand::thread_rng();
        let contract_address = rng.gen();
        let package_address = rng.gen();

        let mut result = NonNull::new(result_ptr).expect("Valid pointer");
        unsafe {
            result.as_mut().contract_address = package_address;
        }

        let mut contracts = self.contracts.write().unwrap();
        contracts.insert(contract_address);

        if let Some(entry_point) = constructor {
            let entry_point = EXPORTS
                .iter()
                .find(|export| export.kind.name().as_bytes() == entry_point)
                .expect("Entry point exists");

            let mut stub = with_current_environment(|stub| stub);
            stub.contract_address = Some(contract_address);
            stub.input_data = input_data.map(Bytes::copy_from_slice);

            // Call constructor, expect a trap
            let result = dispatch_with(stub, || {
                // TODO: Handle panic inside constructor
                (entry_point.fptr)()
            });

            match result {
                Ok(()) => {}
                Err(NativeTrap::Return(flags, bytes)) => {
                    if flags.contains(ReturnFlags::REVERT) {
                        todo!("Constructor returned with a revert flag");
                    }
                    assert!(bytes.is_empty(), "When returning from the constructor it is expected that no bytes are passed in a return function");
                }
                Err(NativeTrap::Panic(_panic)) => {
                    todo!();
                }
            }
        }

        Ok(HOST_ERROR_SUCCESS)
    }

    #[allow(clippy::too_many_arguments)]
    fn casper_call(
        &self,
        address_ptr: *const u8,
        address_size: usize,
        value: *const core::ffi::c_void,
        entry_point_ptr: *const u8,
        entry_point_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8, /* For capturing output
                                                                         * data */
        alloc_ctx: *const core::ffi::c_void,
    ) -> Result<u32, NativeTrap> {
        let address = unsafe { slice::from_raw_parts(address_ptr, address_size) };
        let input_data = unsafe { slice::from_raw_parts(input_ptr, input_size) };
        let entry_point = {
            let entry_point_ptr = NonNull::new(entry_point_ptr as _).expect("Valid pointer");
            let entry_point =
                unsafe { slice::from_raw_parts(entry_point_ptr.as_ptr(), entry_point_size) };
            let entry_point = std::str::from_utf8(entry_point).expect("Valid UTF-8 string");
            entry_point.to_string()
        };

        let value: u128 = {
            let value_ptr = NonNull::new(value as *mut u128).expect("Valid pointer");
            unsafe { *value_ptr.as_ptr() }
        };

        assert_eq!(
            value, 0,
            "Transferred value is not supported in native mode"
        );

        let export = EXPORTS
            .iter()
            .find(|export|
                matches!(export.kind, ExportKind::SmartContract { name, .. } | ExportKind::TraitImpl { name, .. }
                    if name == entry_point)
            )
            .expect("Existing entry point");

        let mut new_stub = with_current_environment(|stub| stub.clone());
        new_stub.input_data = Some(Bytes::copy_from_slice(input_data));
        new_stub.contract_address = Some(address.try_into().expect("Size to match"));

        let ret = dispatch_with(new_stub, || {
            // We need to convert any panic inside the entry point into a native trap. This probably
            // should be done in a more configurable way.
            dispatch_export_call(|| {
                (export.fptr)();
            })
        });

        let unfolded = match ret {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) | Err(error) => Err(error),
        };

        match unfolded {
            Ok(()) => Ok(CALLEE_SUCCEEDED),
            Err(NativeTrap::Return(flags, bytes)) => {
                let ptr = NonNull::new(alloc(bytes.len(), alloc_ctx as _));
                if let Some(output_ptr) = ptr {
                    unsafe {
                        ptr::copy_nonoverlapping(bytes.as_ptr(), output_ptr.as_ptr(), bytes.len());
                    }
                }

                if flags.contains(ReturnFlags::REVERT) {
                    Ok(CALLEE_REVERTED)
                } else {
                    Ok(CALLEE_SUCCEEDED)
                }
            }
            Err(NativeTrap::Panic(panic)) => {
                eprintln!("Panic {panic:?}");
                Ok(CALLEE_TRAPPED)
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
        _env_path: *const u64,
        _env_path_size: usize,
        _alloc: Option<extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8>,
        _alloc_ctx: *const core::ffi::c_void,
    ) -> Result<*mut u8, NativeTrap> {
        todo!()
    }

    fn casper_env_caller(
        &self,
        dest: *mut u8,
        dest_size: usize,
        entity_kind: *mut u32,
    ) -> Result<*const u8, NativeTrap> {
        let dst = unsafe { slice::from_raw_parts_mut(dest, dest_size) };
        let addr = match self.caller {
            Entity::Account(addr) => {
                unsafe {
                    *entity_kind = 0;
                }
                addr
            }
            Entity::Contract(addr) => {
                unsafe {
                    *entity_kind = 1;
                }
                addr
            }
        };

        dst.copy_from_slice(&addr);

        Ok(unsafe { dest.add(32) })
    }
}

thread_local! {
    pub(crate) static LAST_TRAP: RefCell<Option<NativeTrap>> = const { RefCell::new(None) };
    static ENV_STACK: RefCell<VecDeque<Environment>> = RefCell::new(VecDeque::from_iter([
        // Stack of environments has a default element so unit tests do not require extra effort.
        // Environment::default()
    ]));
}

pub fn with_current_environment<T>(f: impl FnOnce(Environment) -> T) -> T {
    ENV_STACK.with(|stack| {
        let stub = {
            let borrowed = stack.borrow();
            let front = borrowed.front().expect("Stub exists").clone();
            front
        };
        f(stub)
    })
}

pub fn current_environment() -> Environment {
    with_current_environment(|env| env)
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

fn dispatch_export_call<F>(func: F) -> Result<(), NativeTrap>
where
    F: FnOnce() + Send + UnwindSafe,
{
    let call_result = panic::catch_unwind(|| {
        func();
    });
    match call_result {
        Ok(()) => {
            let last_trap = LAST_TRAP.with(|last_trap| last_trap.borrow_mut().take());
            match last_trap {
                Some(last_trap) => Err(last_trap),
                None => Ok(()),
            }
        }
        Err(error) => Err(NativeTrap::Panic(error)),
    }
}

fn handle_ret<T: Default>(value: Result<T, NativeTrap>) -> T {
    handle_ret_with(value, || T::default())
}

/// Dispatches a function with a default environment.
pub fn dispatch<T>(f: impl FnOnce() -> T) -> Result<T, NativeTrap> {
    dispatch_with(Environment::default(), f)
}

/// Dispatches a function with a given environment.
pub fn dispatch_with<T>(stub: Environment, f: impl FnOnce() -> T) -> Result<T, NativeTrap> {
    ENV_STACK.with(|stack| {
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
    ENV_STACK.with(|stack| {
        let mut borrowed = stack.borrow_mut();
        borrowed.pop_front();
    });

    result
}

mod symbols {
    // TODO: Figure out how to use for_each_host_function macro here and deal with never type in
    // casper_return
    #[no_mangle]
    /// Read value from a storage available for caller's entity address.
    pub extern "C" fn casper_read(
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        info: *mut ::casper_contract_sdk_sys::ReadInfo,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> u32 {
        let _name = "casper_read";
        let _args = (&key_space, &key_ptr, &key_size, &info, &alloc, &alloc_ctx);
        let _call_result = with_current_environment(|stub| {
            stub.casper_read(key_space, key_ptr, key_size, info, alloc, alloc_ctx)
        });
        crate::casper::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_write(
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
        value_ptr: *const u8,
        value_size: usize,
    ) -> u32 {
        let _name = "casper_write";
        let _args = (&key_space, &key_ptr, &key_size, &value_ptr, &value_size);
        let _call_result = with_current_environment(|stub| {
            stub.casper_write(key_space, key_ptr, key_size, value_ptr, value_size)
        });
        crate::casper::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_print(msg_ptr: *const u8, msg_size: usize) {
        let _name = "casper_print";
        let _args = (&msg_ptr, &msg_size);
        let _call_result = with_current_environment(|stub| stub.casper_print(msg_ptr, msg_size));
        crate::casper::native::handle_ret(_call_result)
    }

    use crate::casper::native::LAST_TRAP;

    #[no_mangle]
    pub extern "C" fn casper_return(flags: u32, data_ptr: *const u8, data_len: usize) {
        let _name = "casper_return";
        let _args = (&flags, &data_ptr, &data_len);
        let _call_result =
            with_current_environment(|stub| stub.casper_return(flags, data_ptr, data_len));
        let err = _call_result.unwrap_err(); // SAFE
        LAST_TRAP.with(|last_trap| last_trap.borrow_mut().replace(err));
    }

    #[no_mangle]
    pub extern "C" fn casper_copy_input(
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> *mut u8 {
        let _name = "casper_copy_input";
        let _args = (&alloc, &alloc_ctx);
        let _call_result =
            with_current_environment(|stub| stub.casper_copy_input(alloc, alloc_ctx));
        crate::casper::native::handle_ret_with(_call_result, ptr::null_mut)
    }

    #[no_mangle]
    pub extern "C" fn casper_create(
        code_ptr: *const u8,
        code_size: usize,
        value: *const core::ffi::c_void,
        constructor_ptr: *const u8,
        constructor_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        result_ptr: *mut ::casper_contract_sdk_sys::CreateResult,
    ) -> u32 {
        let _call_result = with_current_environment(|stub| {
            stub.casper_create(
                code_ptr,
                code_size,
                value,
                constructor_ptr,
                constructor_size,
                input_ptr,
                input_size,
                result_ptr,
            )
        });
        crate::casper::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_call(
        address_ptr: *const u8,
        address_size: usize,
        value: *const core::ffi::c_void,
        entry_point_ptr: *const u8,
        entry_point_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8, /* For capturing output
                                                                         * data */
        alloc_ctx: *const core::ffi::c_void,
    ) -> u32 {
        let _call_result = with_current_environment(|stub| {
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
        crate::casper::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_upgrade(
        _code_ptr: *const u8,
        _code_size: usize,
        _entry_point_ptr: *const u8,
        _entry_point_size: usize,
        _input_ptr: *const u8,
        _input_size: usize,
    ) -> u32 {
        todo!()
    }

    use std::ptr;

    use super::with_current_environment;

    #[no_mangle]
    pub extern "C" fn casper_env_read(
        env_path: *const u64,
        env_path_size: usize,
        alloc: Option<extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8>,
        alloc_ctx: *const core::ffi::c_void,
    ) -> *mut u8 {
        let _name = "casper_env_read";
        let _args = (&env_path, &env_path_size, &alloc, &alloc_ctx);
        let _call_result = with_current_environment(|stub| {
            stub.casper_env_read(env_path, env_path_size, alloc, alloc_ctx)
        });
        crate::casper::native::handle_ret_with(_call_result, ptr::null_mut)
    }

    #[no_mangle]
    pub extern "C" fn casper_env_caller(
        dest: *mut u8,
        dest_len: usize,
        entity: *mut u32,
    ) -> *const u8 {
        let _name = "casper_env_caller";
        let _args = (&dest, &dest_len);
        let _call_result =
            with_current_environment(|stub| stub.casper_env_caller(dest, dest_len, entity));
        crate::casper::native::handle_ret_with(_call_result, ptr::null)
    }
    #[no_mangle]
    pub extern "C" fn casper_env_transferred_value(dest: *mut core::ffi::c_void) {
        let _name = "casper_env_transferred_value";
        let _args = ();
        let _call_result = with_current_environment(|stub| stub.casper_env_transferred_value(dest));
        crate::casper::native::handle_ret(_call_result);
    }
    #[no_mangle]
    pub extern "C" fn casper_env_balance(
        _entity_kind: u32,
        _entity_addr_ptr: *const u8,
        _entity_addr_len: usize,
    ) -> u64 {
        todo!()
    }
    #[no_mangle]
    pub extern "C" fn casper_transfer(
        _entity_kind: u32,
        _entity_addr_ptr: *const u8,
        _entity_addr_len: usize,
        _amount: u64,
    ) -> u32 {
        todo!()
    }

    #[no_mangle]
    pub extern "C" fn casper_env_block_time() -> u64 {
        0
    }

    #[no_mangle]
    pub extern "C" fn casper_emit(
        _topic_ptr: *const u8,
        _topic_size: usize,
        _data_ptr: *const u8,
        _data_size: usize,
    ) -> u32 {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        dispatch_with(Environment::default(), || {
            let msg = "Hello";
            let () = with_current_environment(|stub| stub.casper_print(msg.as_ptr(), msg.len()))
                .expect("Ok");
        })
        .unwrap();
    }

    #[ignore]
    #[test]
    fn test_returns() {
        dispatch_with(Environment::default(), || {
            let _ = with_current_environment(|stub| stub.casper_return(0, ptr::null(), 0));
        })
        .unwrap();
    }
}
