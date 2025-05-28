use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    convert::Infallible,
    fmt,
    panic::{self, UnwindSafe},
    ptr::{self, NonNull},
    slice,
    sync::{Arc, RwLock},
};

use crate::linkme::distributed_slice;
use bytes::Bytes;
use casper_executor_wasm_common::{
    env_info::EnvInfo,
    error::{
        CALLEE_REVERTED, CALLEE_SUCCEEDED, CALLEE_TRAPPED, HOST_ERROR_INTERNAL,
        HOST_ERROR_NOT_FOUND, HOST_ERROR_SUCCESS,
    },
    flags::ReturnFlags,
};
#[cfg(not(target_arch = "wasm32"))]
use rand::Rng;

use super::Entity;
use crate::types::Address;

/// The kind of export that is being registered.
///
/// This is used to identify the type of export and its name.
///
/// Depending on the location of given function it may be registered as a:
///
/// * `SmartContract` (if it's part of a `impl Contract` block),
/// * `TraitImpl` (if it's part of a `impl Trait for Contract` block),
/// * `Function` (if it's a standalone function).
///
/// This is used to dispatch exports under native code i.e. you want to write a test that calls
/// "foobar" regardless of location.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryPointKind {
    /// Smart contract.
    ///
    /// This is used to identify the smart contract and its name.
    ///
    /// The `struct_name` is the name of the smart contract that is being registered.
    /// The `name` is the name of the function that is being registered.
    SmartContract {
        struct_name: &'static str,
        name: &'static str,
    },
    /// Trait implementation.
    ///
    /// This is used to identify the trait implementation and its name.
    ///
    /// The `trait_name` is the name of the trait that is being implemented.
    /// The `impl_name` is the name of the implementation.
    /// The `name` is the name of the function that is being implemented.
    TraitImpl {
        trait_name: &'static str,
        impl_name: &'static str,
        name: &'static str,
    },
    /// Function export.
    ///
    /// This is used to identify the function export and its name.
    ///
    /// The `name` is the name of the function that is being exported.
    Function { name: &'static str },
}

impl EntryPointKind {
    pub fn name(&self) -> &'static str {
        match self {
            EntryPointKind::SmartContract { name, .. }
            | EntryPointKind::TraitImpl { name, .. }
            | EntryPointKind::Function { name } => name,
        }
    }
}

/// Export is a structure that contains information about the exported function.
///
/// This is used to register the export and its name and physical location in the smart contract
/// source code.
pub struct EntryPoint {
    /// The kind of entry point that is being registered.
    pub kind: EntryPointKind,
    pub fptr: fn() -> (),
    pub module_path: &'static str,
    pub file: &'static str,
    pub line: u32,
}

#[distributed_slice]
#[linkme(crate = crate::linkme)]
pub static ENTRY_POINTS: [EntryPoint];

impl fmt::Debug for EntryPoint {
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

/// Invokes an export by its name.
///
/// This function is used to invoke an export by its name regardless of its location in the smart
/// contract.
pub fn invoke_export_by_name(name: &str) {
    let exports_by_name: Vec<_> = ENTRY_POINTS
        .iter()
        .filter(|export| export.kind.name() == name)
        .collect();

    assert_eq!(
        exports_by_name.len(),
        1,
        "Expected exactly one export {name} found, but got {exports_by_name:?}"
    );

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
    caller: Entity,
    callee: Entity,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            db: Default::default(),
            contracts: Default::default(),
            input_data: Default::default(),
            caller: DEFAULT_ADDRESS,
            callee: DEFAULT_ADDRESS,
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
            caller,
            callee: caller,
        }
    }

    #[must_use]
    pub fn with_caller(&self, caller: Entity) -> Self {
        let mut env = self.clone();
        env.caller = caller;
        env
    }

    #[must_use]
    pub fn smart_contract(&self, callee: Entity) -> Self {
        let mut env = self.clone();
        env.caller = self.callee;
        env.callee = callee;
        env
    }

    #[must_use]
    pub fn session(&self, callee: Entity) -> Self {
        let mut env = self.clone();
        env.caller = callee;
        env.callee = callee;
        env
    }

    #[must_use]
    pub fn with_callee(&self, callee: Entity) -> Self {
        let mut env = self.clone();
        env.callee = callee;
        env
    }

    #[must_use]
    pub fn with_input_data(&self, input_data: Vec<u8>) -> Self {
        let mut env = self.clone();
        env.input_data = Some(Bytes::from(input_data));
        env
    }
}

impl Environment {
    fn key_prefix(&self, key: &[u8]) -> Vec<u8> {
        let entity = self.callee;

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

    fn casper_remove(
        &self,
        key_space: u64,
        key_ptr: *const u8,
        key_size: usize,
    ) -> Result<u32, NativeTrap> {
        assert!(!key_ptr.is_null());
        let key_bytes = unsafe { slice::from_raw_parts(key_ptr, key_size) };
        let key_bytes = self.key_prefix(key_bytes);

        let mut db = self.db.write().unwrap();
        if let Some(values) = db.get_mut(&key_space) {
            values.remove(key_bytes.as_slice());
            Ok(HOST_ERROR_SUCCESS)
        } else {
            Ok(HOST_ERROR_NOT_FOUND)
        }
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
        let data = if data_ptr.is_null() {
            Bytes::new()
        } else {
            Bytes::copy_from_slice(unsafe { slice::from_raw_parts(data_ptr, data_len) })
        };
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
        transferred_value: u64,
        constructor_ptr: *const u8,
        constructor_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        seed_ptr: *const u8,
        seed_size: usize,
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

        let _seed = if seed_ptr.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(seed_ptr, seed_size) })
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
            let entry_point = ENTRY_POINTS
                .iter()
                .find(|export| export.kind.name().as_bytes() == entry_point)
                .expect("Entry point exists");

            let mut stub = with_current_environment(|stub| stub);
            stub.input_data = input_data.map(Bytes::copy_from_slice);

            stub.caller = stub.callee;
            stub.callee = Entity::Contract(package_address);

            // stub.callee
            // Call constructor, expect a trap
            let result = dispatch_with(stub, || {
                // TODO: Handle panic inside constructor
                (entry_point.fptr)();
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
        transferred_value: u64,
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
            let entry_point_ptr = NonNull::new(entry_point_ptr.cast_mut()).expect("Valid pointer");
            let entry_point =
                unsafe { slice::from_raw_parts(entry_point_ptr.as_ptr(), entry_point_size) };
            let entry_point = std::str::from_utf8(entry_point).expect("Valid UTF-8 string");
            entry_point.to_string()
        };

        assert_eq!(
            transferred_value, 0,
            "Transferred value is not supported in native mode"
        );

        let export = ENTRY_POINTS
            .iter()
            .find(|export|
                matches!(export.kind, EntryPointKind::SmartContract { name, .. } | EntryPointKind::TraitImpl { name, .. }
                    if name == entry_point)
            )
            .expect("Existing entry point");

        let mut new_stub = with_current_environment(|stub| stub.clone());
        new_stub.input_data = Some(Bytes::copy_from_slice(input_data));
        new_stub.caller = new_stub.callee;
        new_stub.callee = Entity::Contract(address.try_into().expect("Size to match"));

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
                let ptr = NonNull::new(alloc(bytes.len(), alloc_ctx.cast_mut()));
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

    fn casper_env_info(&self, info_ptr: *const u8, info_size: u32) -> Result<u32, NativeTrap> {
        assert_eq!(info_size as usize, size_of::<EnvInfo>());
        let mut env_info = NonNull::new(info_ptr as *mut u8)
            .expect("Valid ptr")
            .cast::<EnvInfo>();
        let env_info = unsafe { env_info.as_mut() };
        *env_info = EnvInfo {
            block_time: 0,
            transferred_value: 0,
            caller_addr: *self.caller.address(),
            caller_kind: self.caller.tag(),
            callee_addr: *self.callee.address(),
            callee_kind: self.callee.tag(),
        };
        Ok(HOST_ERROR_SUCCESS)
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
    pub extern "C" fn casper_remove(key_space: u64, key_ptr: *const u8, key_size: usize) -> u32 {
        let _name = "casper_remove";
        let _args = (&key_space, &key_ptr, &key_size);
        let _call_result =
            with_current_environment(|stub| stub.casper_remove(key_space, key_ptr, key_size));
        crate::casper::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_print(msg_ptr: *const u8, msg_size: usize) {
        let _name = "casper_print";
        let _args = (&msg_ptr, &msg_size);
        let _call_result = with_current_environment(|stub| stub.casper_print(msg_ptr, msg_size));
        crate::casper::native::handle_ret(_call_result);
    }

    use casper_executor_wasm_common::error::HOST_ERROR_SUCCESS;

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
        transferred_value: u64,
        constructor_ptr: *const u8,
        constructor_size: usize,
        input_ptr: *const u8,
        input_size: usize,
        seed_ptr: *const u8,
        seed_size: usize,
        result_ptr: *mut casper_contract_sdk_sys::CreateResult,
    ) -> u32 {
        let _call_result = with_current_environment(|stub| {
            stub.casper_create(
                code_ptr,
                code_size,
                transferred_value,
                constructor_ptr,
                constructor_size,
                input_ptr,
                input_size,
                seed_ptr,
                seed_size,
                result_ptr,
            )
        });
        crate::casper::native::handle_ret(_call_result)
    }

    #[no_mangle]
    pub extern "C" fn casper_call(
        address_ptr: *const u8,
        address_size: usize,
        transferred_value: u64,
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
                transferred_value,
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

    use core::slice;
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
    pub extern "C" fn casper_emit(
        topic_ptr: *const u8,
        topic_size: usize,
        data_ptr: *const u8,
        data_size: usize,
    ) -> u32 {
        let topic = unsafe { slice::from_raw_parts(topic_ptr, topic_size) };
        let data = unsafe { slice::from_raw_parts(data_ptr, data_size) };
        let topic = std::str::from_utf8(topic).expect("Valid UTF-8 string");
        println!("Emitting event with topic: {topic:?} and data: {data:?}");
        HOST_ERROR_SUCCESS
    }

    #[no_mangle]
    pub extern "C" fn casper_env_info(info_ptr: *const u8, info_size: u32) -> u32 {
        let ret = with_current_environment(|env| env.casper_env_info(info_ptr, info_size));
        crate::casper::native::handle_ret(ret)
    }
}

#[cfg(test)]
mod tests {
    use casper_executor_wasm_common::keyspace::Keyspace;

    use crate::casper;

    use super::*;

    #[test]
    fn foo() {
        dispatch(|| {
            casper::print("Hello");
            casper::write(Keyspace::Context(b"test"), b"value 1").unwrap();

            let change_context_1 =
                with_current_environment(|stub| stub.smart_contract(Entity::Contract([1; 32])));

            dispatch_with(change_context_1, || {
                casper::write(Keyspace::Context(b"test"), b"value 2").unwrap();
                casper::write(Keyspace::State, b"state").unwrap();
            })
            .unwrap();

            let change_context_1 =
                with_current_environment(|stub| stub.smart_contract(Entity::Contract([1; 32])));
            dispatch_with(change_context_1, || {
                assert_eq!(
                    casper::read_into_vec(Keyspace::Context(b"test")),
                    Ok(Some(b"value 2".to_vec()))
                );
                assert_eq!(
                    casper::read_into_vec(Keyspace::State),
                    Ok(Some(b"state".to_vec()))
                );
            })
            .unwrap();

            assert_eq!(casper::get_caller(), DEFAULT_ADDRESS);
            assert_eq!(
                casper::read_into_vec(Keyspace::Context(b"test")),
                Ok(Some(b"value 1".to_vec()))
            );
        })
        .unwrap();
    }
    #[test]
    fn test() {
        dispatch_with(Environment::default(), || {
            let msg = "Hello";
            let () = with_current_environment(|stub| stub.casper_print(msg.as_ptr(), msg.len()))
                .expect("Ok");
        })
        .unwrap();
    }

    #[test]
    fn test_returns() {
        dispatch_with(Environment::default(), || {
            let _ = with_current_environment(|stub| stub.casper_return(0, ptr::null(), 0));
        })
        .unwrap();
    }
}
