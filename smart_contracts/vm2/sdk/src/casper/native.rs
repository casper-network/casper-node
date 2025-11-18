use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fmt,
    panic::{self, UnwindSafe},
    slice,
    sync::{Arc, RwLock},
};

use crate::linkme::distributed_slice;
use bytes::Bytes;
use casper_executor_wasm_common::{
    flags::ReturnFlags,
};

use super::Entity;

#[repr(C)]
pub struct Param {
    pub name_ptr: *const u8,
    pub name_len: usize,
}

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
    let all_entry_points = ENTRY_POINTS.iter().collect::<Vec<_>>();

    let exports_by_name: Vec<_> = all_entry_points
        .iter()
        .filter(|export| export.kind.name() == name)
        .collect();

    if exports_by_name.len() != 1 {
        panic!(
            "Expected exactly one export {} found, but got {:?} ({:?})",
            name, exports_by_name, all_entry_points
        );
    }

    let result = dispatch_export_call(exports_by_name[0].fptr);

    match result {
        Ok(()) => {}
        Err(trap) => {
            match trap {
                NativeTrap::Panic(panic_payload) => {
                    // Re-raise the panic so it can be caught by test's #[should_panic]
                    std::panic::resume_unwind(panic_payload);
                }
                other_trap => {
                    // For non-panic traps, set them in LAST_TRAP
                    LAST_TRAP.with(|last_trap| {
                        last_trap.borrow_mut().replace(other_trap);
                    });
                }
            }
        }
    }
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

impl From<&Param> for NativeParam {
    fn from(val: &Param) -> Self {
        let name =
            String::from_utf8_lossy(unsafe { slice::from_raw_parts(val.name_ptr, val.name_len) })
                .into_owned();
        NativeParam(name)
    }
}

#[derive(Clone, Debug)]
pub struct Environment {
    pub db: Arc<RwLock<Container>>,
    input_data: Option<Bytes>,
    caller: Entity,
    callee: Entity,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            db: Default::default(),
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
    #[no_mangle]
    pub extern "C" fn casper_ffi(
        _system_contract_opt: u32,
        _input_ptr: *const u8,
        _input_size: usize,
        _alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        _alloc_ctx: *const core::ffi::c_void,
    ) -> u32 {
        todo!()
    }
}